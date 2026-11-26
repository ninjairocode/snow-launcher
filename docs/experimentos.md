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

### Em aberto

- [x] O Battle.net reconectou sozinho depois desta suspensão curta (cerca de 15 s)? **Sim, aparentemente.**
- [ ] E depois de uma suspensão longa (mais de 10 min), de uma hibernação ou de uma troca de rede (outro Wi-Fi)?
- [ ] Reiniciar o Battle.net e o Agent depois de acordar resolve? E com o WoW aberto?
