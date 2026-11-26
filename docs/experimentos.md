# Experimentos

Resultados dos experimentos da seção 13 do `project.md` e de outras observações feitas na máquina do mantenedor.

## O1 – Processos do Battle.net num prefixo Lutris + GE-Proton

**Data:** 2026-11-26
**Ambiente:** Fedora 45 (kernel 7.3), Lutris com umu-launcher, GE-Proton11-14, prefixo `~/Games/battlenet`, Battle.net aberto e logado.
**Como:** `snowctl status --prefix ~/Games/battlenet` e leitura manual de `/proc/<pid>/{comm,cmdline,environ}`.

### Resultados

1. **O `WINEPREFIX` chega por dois caminhos.** Os processos do lado Linux (Lutris, umu-run) têm `WINEPREFIX=~/Games/battlenet`. Os processos Wine têm `WINEPREFIX=~/Games/battlenet/pfx/`, e `pfx` é um symlink para `.`. **Conclusão:** canonicalizar os dois lados é obrigatório. Isso já está implementado em `ProcfsProcessTable`.
2. **O `comm` não serve para identificar o Battle.net.** O cliente é Chromium (CEF) e renomeia o processo para `CrBrowserMain`, `CrGpuMain`, `CrRendererMain` e `CrUtilityMain`. **Conclusão:** identificar pelo `cmdline`. No Wine, o primeiro argumento é o caminho Windows do executável (`C:\Program Files (x86)\Battle.net\Battle.net.exe`).
3. **Os processos filhos do Chromium** usam o mesmo executável, com `--type=gpu-process`, `--type=renderer` etc. O processo principal é o único sem `--type=`.
4. **O Agent fica numa pasta com versão:** `C:/ProgramData/Battle.net/Agent/Agent.9886/Agent.exe`, com `/` em vez de `\`.
5. **O `cmdline` do Wine é reescrito no próprio lugar** e completado com NULs no fim, então as entradas vazias precisam ser descartadas.
6. **Processos de infraestrutura do Proton** além dos clássicos: `umu.exe`, `lsass.exe` e `xalia.exe`.
7. **Os processos do lado Linux também herdam o `WINEPREFIX`:** `lutris-wrapper`, `umu-run` (python3), `srt-bwrap`, `pv-adverb` e o script `proton`.

### Consequências para o projeto

- **Encerrar "todos os processos do prefixo" inclui o launcher do lado Linux.** No prefixo do próprio Snow isso será o `umu-run` que o próprio Snow iniciou. O encerramento deve começar pelos processos Windows (`wineserver -k` com o mesmo runtime) e deixar que a árvore do launcher termine sozinha. Só depois disso entram `SIGTERM`/`SIGKILL` (seção 9.2). A validar no E4.
- O `environ` de processos de outros usuários dá "permissão negada". Isso é esperado e já é ignorado.

## O2 – Sinal de suspender/acordar do logind (início do E4)

**Data:** 2026-11-26
**Ambiente:** o mesmo do O1, com o Battle.net aberto. `snowd` rodando via `cargo run`, suspensão por `systemctl suspend`.

### Resultados

1. O `PrepareForSleep` chega ao `snowd` sem nenhuma permissão extra, só como usuário comum no barramento do sistema:
   ```
   13:55:59 INFO snowd: system is going to sleep
   13:56:15 INFO snowd: system woke up
   ```
2. Depois de acordar, o Battle.net e o Agent **continuaram com os mesmos PIDs** (97076 e 97140). O problema de "não reconectar" não é processo morto, é o processo vivo sem conexão. Por isso a detecção não pode depender só de "o processo existe".

3. **Rede após acordar:** o NetworkManager voltou a `CONNECTED_GLOBAL` cerca de **5 s** depois do sinal de acordar (suspensão de 13 s, Wi-Fi/cabo sem troca de rede):
   ```
   14:06:59 INFO system woke up, waiting for the network
   14:07:04 INFO network is back after_ms=5016
   ```
   O limite de 60 s do `snowd` deixa folga. Qualquer ação no Battle.net logo após acordar, antes desses ~5 s, encontraria a rede fora.

### Em aberto (O2)

- [x] O Battle.net reconectou sozinho depois desta suspensão curta (cerca de 15 s)? **Sim, aparentemente.**
- [ ] E depois de uma suspensão longa (mais de 10 min), de uma hibernação ou de uma troca de rede (outro Wi-Fi)?
- [ ] Reiniciar o Battle.net e o Agent depois de acordar resolve? E com o WoW aberto?

## O3 – Bloquear suspensão automática com o WoW aberto

**Data:** 2026-11-26
**Ambiente:** o mesmo do O1. `snowd --prefix ~/Games/battlenet`, sem `config.toml` (padrão), WoW retail aberto pelo Battle.net.

### Resultados

1. O WoW retail foi reconhecido como jogo (`Wow.exe`) e a inibição foi aplicada e liberada com o jogo aberto e fechado:
   ```
   14:41:14 INFO game running games=["World of Warcraft"]
   14:41:14 INFO automatic suspend blocked
   14:41:49 INFO no game running
   14:41:49 INFO automatic suspend allowed again
   ```
2. No GNOME, o portal registra a inibição com a flag *Suspend* (4) e sem *Idle* (8), conforme o teste `real_portal_inhibits_and_releases_suspend_on_gnome`. O registro leva ~300 ms para aparecer.

3. **Suspensão manual curta (14 s) com o WoW aberto:** a notificação apareceu ao acordar, mas **o jogo continuou online**. Foi um falso positivo. A rede voltou em 8,6 s.
   ```
   14:51:57 INFO system is going to sleep
   14:52:11 INFO system woke up
   14:52:11 INFO games were running during suspend; warning the user games=["World of Warcraft"]
   14:52:20 INFO network is back after_ms=8632
   ```
   **Conclusão:** avisar só porque havia jogo aberto não basta. É preciso verificar se a conexão do jogo realmente morreu. Daí a checagem das conexões TCP do jogo em `/proc/<pid>/net/tcp`: estado, fila de envio e contador de retransmissões.

4. **Mesma suspensão curta (16 s), já com a checagem de conexões:** nenhum aviso, que é o correto.
   ```
   15:08:18 INFO network is back after_ms=12562
   15:08:18 INFO checking whether the game connection survived games=["World of Warcraft"]
   15:09:18 INFO game connection survived the suspend games=["World of Warcraft"]
   ```
5. **Cabo de rede removido e recolocado com o WoW aberto:** o jogo se reconectou sozinho. Uma conexão que retransmite porque a *rede local* caiu não está morta: ela se recupera se o servidor ainda mantém a sessão. **Conclusão:** as retransmissões só podem contar enquanto a rede estiver online.

6. **Conexão meio aberta sem suspensão:** depois de uma queda de rede mais longa, a rede voltou (NetworkManager `State = 70`, conexões HTTPS normais), mas o WoW "não se comunicava com mais nada", embora o personagem ainda andasse. As duas conexões com a **porta 3724** (servidor de jogo; IPs 24.105.52.59 e 66.40.179.35) estavam com **10 retransmissões**, timer de retransmissão ligado e fila de envio crescendo (0x138 → 0x154 bytes em 5 s):
   ```
   rem=3B346918:0E8C st=01 tx:rx=00000138:00000000 tr:when=01:00000F2A retrnsmt=0000000A
   rem=23B32842:0E8C st=01 tx:rx=0000013C:00000000 tr:when=01:00000F2A retrnsmt=0000000A
   ```
   **Conclusão:** o problema não é exclusivo da suspensão. Qualquer queda longa o bastante deixa o jogo assim. A vigilância das conexões passou a rodar durante toda a sessão de jogo, com limite de 6 retransmissões.

### Em aberto

- [ ] Por quanto tempo o servidor do WoW mantém a sessão sem resposta? (Tirar o cabo e cronometrar até o jogo mostrar a desconexão, ou até ele voltar ao recolocar.)
- [ ] Suspensão longa com o WoW aberto: o jogo de fato perde a conexão? Em quanto tempo ele mesmo percebe? A checagem de retransmissões detecta isso?
- [ ] Deixar o PC ocioso além do tempo de suspensão automática com o WoW aberto: confirmar que a tela apaga mas o PC não suspende.
