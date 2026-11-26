//! TCP connections of a process, and whether they died.
//!
//! If the network is gone for long enough (a suspend, a pulled cable, a
//! router restart), the game server drops the session. When the network
//! comes back the game keeps a "half-open" connection: it sends data, gets
//! no answer, and the kernel keeps retransmitting for ~15 minutes before
//! giving up. Meanwhile the game looks alive (you can even walk around)
//! but talks to nothing. That retransmission is visible, without root, in
//! `/proc/<pid>/net/tcp`.

use std::collections::HashSet;

use crate::desktop::Notification;
use crate::process::Pid;

/// `TCP_ESTABLISHED` in `/proc/net/tcp`'s `st` column.
const STATE_ESTABLISHED: u8 = 0x01;
/// `tr` column: 1 = the retransmission timer is running.
const TIMER_RETRANSMIT: u8 = 1;
/// Consecutive retransmissions of the same data with no answer from the
/// server before a connection is considered dead. With exponential
/// backoff, 6 retransmissions take roughly 20 to 30 seconds of silence,
/// so a brief Wi-Fi hiccup does not count. A real half-open connection
/// observed with WoW had reached 10.
pub const DEAD_AFTER_RETRANSMITS: u32 = 6;

/// One line of `/proc/<pid>/net/tcp` or `tcp6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpSocket {
    pub inode: u64,
    pub state: u8,
    /// Bytes sent but not yet acknowledged by the other side.
    pub tx_queue: u32,
    pub timer: u8,
    /// Retransmissions of the oldest unacknowledged data. Resets when the
    /// other side answers.
    pub retransmits: u32,
}

impl TcpSocket {
    /// Established, with unacknowledged data being retransmitted again
    /// and again: the other side is not answering.
    pub fn looks_dead(&self) -> bool {
        self.state == STATE_ESTABLISHED
            && self.tx_queue > 0
            && self.timer == TIMER_RETRANSMIT
            && self.retransmits >= DEAD_AFTER_RETRANSMITS
    }
}

/// Parses the text of `/proc/<pid>/net/tcp` or `tcp6`.
/// The header and malformed lines are skipped.
pub fn parse_proc_net_tcp(text: &str) -> Vec<TcpSocket> {
    text.lines().skip(1).filter_map(parse_line).collect()
}

/// Columns: `sl local rem st tx:rx tr:when retrnsmt uid timeout inode ...`
/// All numbers are hexadecimal except `uid`, `timeout` and `inode`.
fn parse_line(line: &str) -> Option<TcpSocket> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let hex_u32 = |s: &str| u32::from_str_radix(s, 16).ok();
    let hex_u8 = |s: &str| u8::from_str_radix(s, 16).ok();

    let state = hex_u8(fields.get(3)?)?;
    let (tx, _rx) = fields.get(4)?.split_once(':')?;
    let (timer, _when) = fields.get(5)?.split_once(':')?;
    Some(TcpSocket {
        state,
        tx_queue: hex_u32(tx)?,
        timer: hex_u8(timer)?,
        retransmits: hex_u32(fields.get(6)?)?,
        inode: fields.get(9)?.parse().ok()?,
    })
}

/// Source of a process's TCP sockets. The real implementation reads
/// `/proc`; tests use fixed lists.
pub trait SocketTable {
    type Error: std::error::Error + Send + Sync + 'static;

    /// TCP sockets (IPv4 and IPv6) owned by `pid`, or `None` if the
    /// process no longer exists.
    fn tcp_sockets(&self, pid: Pid) -> Result<Option<Vec<TcpSocket>>, Self::Error>;
}

/// Decides, sample after sample, whether a connection died.
///
/// A connection counts as dead only if it looks dead in **two consecutive
/// samples**. Right after the network comes back, a connection may still
/// show retransmissions from while it was offline; if it is alive, the
/// next answer from the server resets them.
#[derive(Debug, Default)]
pub struct DeadConnectionDetector {
    previously_dead: HashSet<u64>,
}

impl DeadConnectionDetector {
    /// Feeds one sample of sockets. Returns `true` once a connection has
    /// looked dead twice in a row.
    pub fn observe(&mut self, sockets: &[TcpSocket]) -> bool {
        let dead: HashSet<u64> = sockets
            .iter()
            .filter(|socket| socket.looks_dead())
            .map(|socket| socket.inode)
            .collect();
        let confirmed = !dead.is_disjoint(&self.previously_dead);
        self.previously_dead = dead;
        confirmed
    }

    /// Forgets earlier samples. Used when the local network drops: while
    /// it is down, retransmissions are expected and say nothing about the
    /// server.
    pub fn reset(&mut self) {
        self.previously_dead.clear();
    }
}

/// The warning shown when a game's connection died, or `None` if no
/// game is given.
// TODO(i18n): translate (pt-BR) once gettext is in place (v0.6).
pub fn lost_connection_notice(game_titles: &[&str]) -> Option<Notification> {
    let (summary, which) = match game_titles {
        [] => return None,
        [title] => (format!("{title} lost its connection"), "The game is"),
        _ => (
            format!("{} lost their connection", game_titles.join(", ")),
            "The games are",
        ),
    };
    let body = format!(
        "The server stopped answering. {which} still open but no longer \
         connected, and may take several minutes to notice. Close it and \
         open it again."
    );
    Some(Notification {
        summary,
        body,
        urgent: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // HEALTHY and CLOSE_WAIT follow lines seen on Battle.net under GE-Proton.
    // STUCK is hand-made: unacked data (tx_queue), retransmit timer on (tr=01)
    // and 6 retransmissions, as a half-open connection would show.
    const HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";
    const HEALTHY: &str = "   5: 0F02000A:C5A2 52861022:045F 01 00000000:00000000 00:00000000 00000000  1000        0 774411 1 0000000000000000 20 4 30 10 -1";
    const CLOSE_WAIT: &str = "   6: 0F02000A:D1C0 00A75363:01BB 08 00000000:00000020 00:00000000 00000000  1000        0 774500 1 0000000000000000 20 4 30 10 -1";
    const STUCK: &str = "   7: 0F02000A:E010 52861022:0D3D 01 000000A0:00000000 01:000007D0 00000006  1000        0 774600 2 0000000000000000 1600 4 30 10 -1";

    fn sockets(lines: &[&str]) -> Vec<TcpSocket> {
        parse_proc_net_tcp(&[&[HEADER], lines].concat().join("\n"))
    }

    #[test]
    fn parses_real_lines() {
        let parsed = sockets(&[HEALTHY, STUCK]);
        assert_eq!(
            parsed,
            [
                TcpSocket {
                    inode: 774411,
                    state: 0x01,
                    tx_queue: 0,
                    timer: 0,
                    retransmits: 0,
                },
                TcpSocket {
                    inode: 774600,
                    state: 0x01,
                    tx_queue: 0xA0,
                    timer: 1,
                    retransmits: 6,
                },
            ]
        );
    }

    #[test]
    fn skips_header_and_garbage() {
        assert!(sockets(&["", "garbage", "1: x y"]).is_empty());
    }

    #[test]
    fn only_silent_retransmitting_connections_look_dead() {
        let parsed = sockets(&[HEALTHY, CLOSE_WAIT, STUCK]);
        let dead: Vec<u64> = parsed
            .iter()
            .filter(|s| s.looks_dead())
            .map(|s| s.inode)
            .collect();
        assert_eq!(dead, [774600]);
    }

    #[test]
    fn few_retransmissions_are_not_enough() {
        let mut socket = sockets(&[STUCK])[0];
        socket.retransmits = DEAD_AFTER_RETRANSMITS - 1;
        assert!(!socket.looks_dead());
    }

    #[test]
    fn detector_needs_two_consecutive_dead_samples() {
        let stuck = sockets(&[STUCK]);
        let mut detector = DeadConnectionDetector::default();
        assert!(!detector.observe(&stuck)); // first sight: not yet
        assert!(detector.observe(&stuck)); // still stuck: dead
    }

    #[test]
    fn detector_forgives_a_connection_that_recovers() {
        let stuck = sockets(&[STUCK]);
        let mut recovered = stuck.clone();
        recovered[0].retransmits = 0;
        recovered[0].timer = 0;
        recovered[0].tx_queue = 0;

        let mut detector = DeadConnectionDetector::default();
        assert!(!detector.observe(&stuck));
        assert!(!detector.observe(&recovered)); // server answered
        assert!(!detector.observe(&stuck)); // needs two in a row again
    }

    #[test]
    fn reset_forgets_previous_samples() {
        let stuck = sockets(&[STUCK]);
        let mut detector = DeadConnectionDetector::default();
        assert!(!detector.observe(&stuck));
        detector.reset();
        assert!(!detector.observe(&stuck));
        assert!(detector.observe(&stuck));
    }

    #[test]
    fn no_notice_without_games() {
        assert_eq!(lost_connection_notice(&[]), None);
    }

    #[test]
    fn notice_names_the_game_and_is_urgent() {
        let notice = lost_connection_notice(&["World of Warcraft"]).unwrap();
        assert_eq!(notice.summary, "World of Warcraft lost its connection");
        assert!(notice.body.contains("The game is still open"));
        assert!(notice.body.contains("Close it"));
        assert!(notice.urgent);
    }

    #[test]
    fn notice_lists_several_games() {
        let notice = lost_connection_notice(&["World of Warcraft", "Diablo IV"]).unwrap();
        assert_eq!(
            notice.summary,
            "World of Warcraft, Diablo IV lost their connection"
        );
        assert!(notice.body.contains("The games are"));
    }
}
