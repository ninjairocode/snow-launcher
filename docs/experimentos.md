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

7. **Vigilância contínua, cabo fora por ~90 s** (`RUST_LOG=snowd=debug`): durante a queda, as amostras pararam de contar (`network is down`). Quando a rede voltou, todas as conexões do WoW estavam saudáveis (`max_retransmits=0`) e **não houve aviso**, que é o correto. O jogo manteve a sessão.
   ```
   16:51:37 DEBUG game connection sample sockets=16 looking_dead=0 max_retransmits=3
   16:51:42 DEBUG network is down; game retransmissions do not count
   16:53:12 DEBUG game connection sample sockets=14 looking_dead=0 max_retransmits=0
   ```
   **Conclusão parcial:** o servidor do WoW tolera pelo menos ~90 s sem resposta. A sessão meio aberta do item 6 veio de uma queda mais longa.

### Em aberto

- [ ] Por quanto tempo o servidor do WoW mantém a sessão? Já se sabe que é **pelo menos ~90 s**; falta achar o limite.
- [ ] Confirmar que a vigilância contínua avisa num caso real de conexão meio aberta (queda longa).
- [ ] Suspensão longa com o WoW aberto: o jogo de fato perde a conexão? Em quanto tempo ele mesmo percebe? A checagem de retransmissões detecta isso?
- [ ] Deixar o PC ocioso além do tempo de suspensão automática com o WoW aberto: confirmar que a tela apaga mas o PC não suspende.

## O4 – Erro do Agent (`BLZBNTBNA00000005`) em toda inicialização

**Data:** 2026-11-27
**Ambiente:** o mesmo do O1 (Lutris + umu + GE-Proton11-14), Battle.net 2.53.8.
**Como:** `snowctl doctor --sessions 20`; depois, Battle.net aberto com acompanhamento dos PIDs (`snowctl status` a cada 5 s) e dos logs do cliente e do Agent.

### Resultados

1. O código aparece em **15 de 15** sessões guardadas, sempre uma vez, cerca de 45 a 65 s depois de o Battle.net abrir, como uma linha de debug do `UrlManager` (`Resolved URL: key=/client/error/BLZBNTBNA00000005`). É o único código de erro resolvido nessas sessões.
2. Linha do tempo de uma abertura (horário local):
   ```
   13:14:34  Battle.net abre
   13:14:37  Agent abre (PID 33080)
   13:15:39.598  cliente registra BLZBNTBNA00000005
   13:15:39.635  cliente cria uma janela nova (EnableNonClientDpiScaling ... for HWND)
   ```
   O Agent **continuou vivo** (mesmo PID) e o log dele **não tem nenhum aviso ou erro** nesse minuto.
3. **Nada apareceu na tela** e o Battle.net funcionou normalmente.

### Conclusão

Sob o Wine, o cliente acha que o Agent não respondeu a tempo enquanto ele ainda está subindo, registra o erro e se recupera sozinho. O erro só importa junto com um sintoma (mensagem visível; instalar, atualizar ou jogar não funciona). O `doctor` passou a marcá-lo como **"usually harmless"**, e a correção automática da seção 8 só deve agir com sintoma, nunca só porque o código está no log.

Também apareceu uma vez o `BLZBNTBGS80000011` (2026-11-23), ainda sem explicação no catálogo.

## O5 – NTSYNC, unidades e ajustes do prefixo atual (E1, E2, E3)

**Data:** 2026-11-27
**Ambiente:** o mesmo do O1 (kernel 7.3.6, Lutris + umu + GE-Proton11-14), com o Battle.net aberto.
**Como:** só leitura: `/dev/ntsync`, `lsmod`, `/proc/<wineserver>/fd` e `environ`, `dosdevices/`, `user.reg`, `Battle.net.config`.

### Resultados

1. **E2 (NTSYNC): funciona e já está em uso.** `/dev/ntsync` existe com permissão `crw-rw-rw-`, o módulo `ntsync` está carregado e o `wineserver` do Battle.net tinha **2184 descritores abertos em `/dev/ntsync`**. Isso sem nenhuma configuração: o GE-Proton11-14 usa o NTSYNC sozinho quando o dispositivo está acessível, mesmo com `WINEESYNC=1` e `WINEFSYNC=1` no ambiente (passados pelo Lutris).
   - **Para o Snow:** conferir o NTSYNC de verdade é olhar o `/proc/<wineserver>/fd`, não as variáveis de ambiente. Isso pode virar um item da barra de status e do `doctor`.
2. **E3 (unidade / versão do Windows):** o prefixo do Lutris **não tem `G:`**: as unidades são `c:`, `s:` (`/home`), `v:` (`/run/media`), `x:` (home do usuário) e `z:` (`/`). O WoW está **dentro** do prefixo, em `drive_c/Program Files (x86)/World of Warcraft` (49 GB). Não há versão do Windows forçada para o Battle.net; as entradas de `AppDefaults` no `user.reg` são padrões do Proton para outros jogos.
3. **E1 (aceleração por hardware):** o `Battle.net.config` não tem a opção, então ela está no padrão (ligada). Ainda falta comparar ligada e desligada.

### Em aberto

- [ ] E1: desligar "aceleração por hardware do navegador" nas configurações do Battle.net e comparar a fluidez da interface.
- [ ] E3: a loja dentro do WoW funciona hoje, com o jogo em `C:` e sem `G:`?

## O6 – E6: casinha via umu, WoW em `G:` e a loja

**Data:** 2026-11-27
**Ambiente:** o mesmo do O1. Prefixo de teste novo em `~/.local/share/snow-launcher/prefixes/e6-test`, criado com o `umu-run` do Lutris, `GAMEID=umu-default` e `PROTONPATH` fixado no GE-Proton11-14. O prefixo do Lutris ficou intocado, e os dois Battle.net nunca rodaram juntos.

### Resultados

1. **Prefixo novo:** `umu-run ""` criou o prefixo em **4 s**, com o mesmo layout do Lutris (`pfx -> .`). O Proton informou `ntsync: up and running`.
2. **Instalador oficial** (`Battle.net-Setup.exe`, assinado por *Blizzard Entertainment, Inc.*, cadeia DigiCert Trusted G4; checagem parcial, só do certificado): **não travou**. Em ~78 s (14:20:19 → 14:21:37), o Agent subiu, os arquivos chegaram, o instalador fechou sozinho e o Battle.net abriu.
3. **Biblioteca em `G:`:** `dosdevices/g:` apontando para a pasta que já contém `World of Warcraft` (49 GB), sem mover nada. O "Localizar o jogo" em `G:\World of Warcraft` **reaproveitou a instalação**: aplicou só uma atualização pendente (49 arquivos tocados em 13 s, quase todos em `Data/data/`) e o jogo apareceu pronto para jogar. O `product.db` do Battle.net de teste registra `G:/World of Warcraft`.
4. **Jogo:** abriu e rodou normalmente com `G:\World of Warcraft\_forever_\Wow.exe -launcherlogin -uid wowf`. O produto instalado é o **`wowf`** (pasta `_forever_`), dado útil para o E5.
5. **Loja do jogo:**
   - Prefixo novo, que informa "Windows 10 Pro": **ficou em "conectando"**.
   - Ao mesmo tempo, pelo **Lutris**, que rodou `winetricks win11` e informa "Microsoft Windows 11": **abriu**.
   - Prefixo novo depois de `winecfg -v win11` (só essa mudança): **abriu**.

   **Forte indício** de que a loja depende de o Wine informar **Windows 11**. Não foi confirmado com o teste inverso (voltar para win10 e a loja falhar de novo): a primeira execução do Battle.net recém-instalado também pode ter influído.
6. **Detalhe do registro:** o prefixo do Lutris informa "Windows 11" com build **19045** (de Windows 10), uma combinação inconsistente. O `winecfg -v win11` deixou tudo consistente (build 22000).

### Escala da interface (DPI)

- O instalador e o Battle.net ficam **pequenos** num prefixo novo: o Wine usa **96 DPI** e não acompanha a escala do desktop.
- Ambiente: monitor 2560×1440, GNOME com escala **125%** (Wayland). O GNOME informa às aplicações X (Xwayland) **`Xft.dpi = 192`**: com escala fracionária, os aplicativos X são desenhados em 2× e reduzidos.
- No Lutris, o mantenedor usa a opção DPI com **144** (150%) e acha legível.
- **Hipótese:** usar o `Xft.dpi` do desktop (192) deixaria o Wine do mesmo tamanho que os outros aplicativos. A validar no E8.

### Conclusões para a v0.2

- O fluxo "prefixo novo via umu + instalador oficial + biblioteca em `G:` + Localizar jogo" **funciona**, sem baixar o jogo de novo.
- Ajustes padrão da casinha: **Windows 11** (forte indício, loja) e **DPI automático** (E8).

### Em aberto

- [ ] Confirmar a versão do Windows com o teste inverso (win10 → a loja falha de novo?).
- [ ] E8: comparar DPI 144 e 192 no Battle.net e no instalador.
