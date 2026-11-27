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

- [ ] E1: desligar "aceleração por hardware do navegador" nas configurações do Battle.net e comparar a fluidez da interface. **Primeira impressão (2026-11-27): sem diferença clara.** Precisa de mais uso, ou de uma medida objetiva, antes de virar ajuste padrão.
- [ ] E3: a loja dentro do WoW funciona hoje, com o jogo em `C:` e sem `G:`? **Ultimamente abre dentro do jogo**, mas o comportamento varia; acompanhar no dia a dia antes de concluir que a correção da letra de unidade é desnecessária.

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

## O7 – E8: DPI do Wine igual ao do desktop

**Data:** 2026-11-27
**Ambiente:** o mesmo do O6 (monitor 2560×1440, GNOME Wayland com escala 125%, `Xft.dpi = 192`), no prefixo de teste.
**Como:** as três chaves `LogPixels` que o Lutris usa (`HKCU\Control Panel\Desktop`, `HKCU\Software\Wine\Fonts` e `HKLM\System\CurrentControlSet\Hardware Profiles\Current\Software\Fonts`) gravadas com **192** (`0xc0`) por um `.reg` importado com `regedit /S`, com backup antes. Depois, o Battle.net foi aberto.

### Resultado

- Com **192**, o Battle.net ficou **perfeito** para o mantenedor: legível e do tamanho certo. Antes, usava 144 no Lutris e 96 no padrão do prefixo novo.
- Confirma a ideia de usar o `Xft.dpi` do desktop como padrão automático.

### Em aberto

- [ ] O instalador com 192.
- [ ] Se o DPI afeta algo dentro do WoW.
- [ ] Outras escalas e ambientes (GNOME 100%/150%/200%, KDE nos dois modos, sessão X11, vários monitores): pedir relatos de usuários.

## O8 – Controle do Battle.net por fora (E5, E9, E10)

**Data:** 2026-11-27
**Ambiente:** prefixo de teste do O6 (Windows 11, DPI 192), Battle.net 2.53.8, GE-Proton11-14. Comandos auxiliares rodados no mesmo prefixo com `PROTON_VERB=runinprefix`, senão o Proton espera o prefixo inteiro terminar.

### E10: etapas da inicialização e fechamento

1. **Marcas de etapa no log do cliente** (sessão com login automático):

   | Tempo | Marca | Etapa |
   |---|---|---|
   | 0 s | `[Main] Logging started for Battle.net build …` | iniciando |
   | +2,9 s | `[Agent] AgentManager is now initialized` | Agent pronto |
   | +5,5 s | `[LoginController] Login_QueueUpdate …` | login em andamento |
   | +5,7 s | `[GSAccountProvider] Login triggered.` | login em andamento |
   | +6,2 s | `[GSStateTracker] State changed to: Ready` | **pronto** |
   | +6,3 s | `[BNLogin] Logged into Battle.net successfully.` | **pronto** |

   "Aguardando login" (sem sessão salva) não foi observado: fica como "iniciou e não chegou a `Ready` em N segundos", como a seção 5 já previa. O `BLZBNTBNA00000005` do O4 é registrado no mesmo instante do `Ready`.
2. **Fechar por fora:** `taskkill /im Battle.net.exe` **sem `/f`** (pedido normal de fechar) fechou o Battle.net **de verdade**, em ~1 s, com desmontagem ordenada no log (`Destroying ObserverRecord …`). O Agent saiu sozinho ~10 s depois (`No Connected Clients detected: Shutting Down`, `Agent is shutting down`). Prefixo vazio, sem `wineserver -k` nem sinais.

### E9: bandeja e reabrir a janela

1. O "X" da janela do Battle.net **só esconde na bandeja**; o processo continua.
2. Rodar o `Battle.net.exe` **de novo** com ele na bandeja: a segunda execução terminou em ~1 s, **sem criar outra instância** (mesmo PID principal), e **a janela voltou** à tela.
3. Ainda não foi testado como **esconder o ícone do Battle.net** na bandeja. A condição de segurança (o Snow conseguir trazer a janela de volta) está validada.

### E5: abrir o jogo direto

1. `Battle.net.exe --exec="launch wowf"` (o *uid* do Agent): **ignorado**, sem erro e sem nada no log.
2. `Battle.net.exe --exec="launch WoWF"` (o **código do produto**): o `Wow.exe` subiu **4 s** depois, com a mesma linha de comando do botão "Jogar" (`G:\World of Warcraft\_forever_\Wow.exe -launcherlogin -uid wowf`), e o jogo abriu normalmente.
3. O log do cliente liga os dois: `productCode=WoWF_wowf`. O Snow pode **descobrir o código pelo log**, sem depender só de uma lista fixa, e o catálogo de jogos guarda o código de produto **e** o *uid*.
4. Ainda não testado: se o jogo herda o ambiente do Battle.net (variáveis do MangoHud, gamescope), a outra metade do E5.

## O9 – A casinha de verdade, a cópia por reflink e o Qt a 192 DPI (E11)

**Data:** 2026-11-27
**Ambiente:** Fedora 45, GNOME Wayland a 125% (`Xft.dpi = 192`), GE-Proton11-14, `/home` em btrfs (LUKS).

### Criação e importação

1. `snowctl house create` criou a casinha em `~/.local/share/snow-launcher/prefixes/battlenet` em **13 s**: prefixo, Windows 11, DPI 192 nas três chaves e `G:` apontando para `~/Games/SnowLauncher`, com cada passo verificado no registro.
2. **Importação por reflink:** com o Lutris fechado, `cp -a --reflink=always` copiou o WoW (**49,6 GB, 4.942 arquivos**) para a biblioteca em **1 s**. A comparação arquivo a arquivo (caminho e tamanho) foi idêntica, e a cópia ocupou só **14 MB**. O Lutris continua funcionando com o original.

### Instalação do Battle.net e o DPI

3. **Com DPI 192, o instalador travou** na tela de idioma, com ~190% de CPU, sem abrir o Agent e sem criar arquivos. O vigia do `install-battlenet` reportou "*no progress for 5 minutes and seems stuck*" e não fechou nada. O instalador fechou com o `taskkill` sem `/f`.
4. **Com DPI 96 (Windows 11 mantido), o instalador funcionou**, porém pequeno: o Agent subiu, o Battle.net foi instalado (779 MB) e abriu.
5. **Com DPI 192, o Battle.net abriu, mas quebrou na etapa da senha do login** (`UAuth … state=LoginCredential`), com violação de acesso (`0xc0000005`) na thread principal, dentro de `Qt5Core.dll`/`Qt5Widgets.dll`. A 144 o login completou, e a sessão ficou salva.
6. **Ponto comum:** o instalador e a janela de login são Qt (**Qt 5.15.1**), e o log mostra o Qt chamando `EnableNonClientDpiScaling()`, que o Wine não implementa. A janela principal é CEF e funciona a 192, o que explica o E8: lá havia sessão salva e a janela de senha não apareceu.

### E11: a causa e a correção

7. Num prefixo descartável igual à casinha (Windows 11, DPI 192), com **`QT_ENABLE_HIGHDPI_SCALING=0`** no ambiente:
   - o **instalador funcionou** e ficou **legível a 192**;
   - o Battle.net aberto por ele (herdando a variável) **completou o login com senha a 192**, sem quebrar.
8. **Conclusão:** o modo de escala para alta densidade do Qt 5.15 trava ou quebra sob o Wine acima de 100%. Desligado, o Qt continua usando o DPI do Wine para textos e elementos. A variável virou o ambiente padrão da casinha (`HOUSE_ENV`) e é aplicada em tudo o que o Snow roda nela.
9. O prefixo descartável foi apagado logo depois, com a sessão de login.

### Abrir a casinha

10. `snowctl house open`: com o Battle.net **na bandeja**, trouxe a janela de volta em **1 s**, no mesmo processo. Com ele **fechado**, abriu em **4 s**, legível a 192 e sem erro. O `QT_ENABLE_HIGHDPI_SCALING=0` estava no ambiente do processo, e o `umu-run` ficou num grupo de processos próprio (fechar o terminal não derruba o Battle.net).
11. O "Localizar o jogo" registrou `G:/World of Warcraft`. O Battle.net reconheceu a cópia e tocou em 14 arquivos.

### Lições

- Um `regedit /S` de um arquivo **inexistente** termina com código 0 sem fazer nada (aconteceu quando o scratchpad foi limpo entre sessões). Verificar o resultado no registro, como o `house create` faz, é indispensável.
- Backups de experimentos vão para `~/.local/state/snow-launcher/`, não para pastas temporárias.

## O10 – E7: AppImage com GTK4/libadwaita

**Data:** 2026-11-27
**Como:** app mínimo Relm4 0.10 + libadwaita-rs 0.8 (Rust 1.85, binário de 708 KB) com a ilustração "Sobre" embutida; AppImage montado **sem o `appimagetool`**: `mksquashfs` (squashfs-tools do Fedora) + `type2-runtime` tag `20251108`; testes em contêineres podman com a janela no Wayland do host.

### Ferramentas e verificação

1. O `appimagetool` (incluindo a tag `1.9.1`) **não publica hash nem assinatura**. As compilações "contínuas" baixadas no início foram descartadas sem executar.
2. O `type2-runtime` `20251108` publica assinatura GPG. A chave do lançamento é idêntica à do código-fonte na mesma tag (ed25519, `570C 77AC EA40 C0F1 B758 902C BF96 CCA5 6490 F695`). A verificação deu **"Good signature"**, e com 1 byte alterado, **"BAD signature"**. O runtime é `static-pie` e **não depende de `libfuse2`**.

### Resultados (caminho A: GTK do sistema)

3. AppImage de **1,2 MB**. O binário exige no máximo **glibc 2.34** e usa só a API base do GTK (29 funções).
4. **Fedora 45** (nativo, montando via FUSE): GTK 4.24.2 / libadwaita 1.10.0. ✅
5. **Arch** (contêiner): 4.24.2 / 1.10.0. ✅
6. **Ubuntu 24.04** (contêiner): 4.14.5 / 1.5.0. Rodou, mas **o layout quebrou**: o título do `adw::StatusPage` saiu uma letra por linha e a imagem sumiu.
   - Não era falta de driver: com Mesa, o GTK usou a GPU, e o problema continuou.
   - Não era a imagem: sem ela, o mesmo aconteceu.
   - Um relatório das larguras reais mostrou o conteúdo do `StatusPage` com 556 px de altura e o `GtkPicture` com **altura 0**. Com `ToolbarView` + `StatusPage`, igual.
   - **Sem o `StatusPage`** (um `gtk::Label` com quebra de linha e o `GtkPicture`): título numa linha (520x32) e imagem 520x358, **confirmado visualmente**. ✅
7. **Conclusão:** o caminho A funciona. Base mínima adotada: **Ubuntu 24.04** (GTK 4.14, libadwaita 1.5).
   - **Corrigido no O11:** a "armadilha de layout por versão" estava errada. A causa era o contêiner sem `gtk-xft-dpi`, e não a libadwaita 1.5.

### Não testado

- Ubuntu de verdade (com GPU e FUSE; nos contêineres foi usado o modo *extract-and-run*), Ubuntu 22.04 (fora da base) e sessão X11.

## O11 – Correção do E7: o `Clamp` sem DPI de texto, e não a libadwaita 1.5

**Data:** 2026-11-27
**Contexto:** a primeira janela da GUI (`snowlauncher-gui`, com `ScrolledWindow` + `adw::Clamp` e **sem** `StatusPage`) ficou perfeita no Fedora, mas no contêiner Ubuntu 24.04 toda a coluna de conteúdo encolheu para ~110 px: texto quebrando a cada palavra, ilustração pequena e lista espremida. Era o mesmo sintoma do `StatusPage` no O10, sem usar o `StatusPage`.

### Resultados

1. No contêiner, o GTK enxerga **`gtk-xft-dpi = -1`** (não definido): não há o serviço de configurações do GNOME.
2. Desde a libadwaita 1.4, o `adw::Clamp` mede a largura máxima em **"sp"** e converte para pixels com esse DPI. Com o DPI não definido, a conversão quebra:
   - `Clamp` em "sp": título com **91 px** de largura;
   - `Clamp` em **pixels**: título com **502x32**, normal.
3. **Com o DPI definido** (um `settings.ini` do GTK com `gtk-xft-dpi=98304`, como num desktop de verdade), no mesmo Ubuntu 24.04:
   - o `StatusPage` do O10 **funciona** (`Clamp` 496x62, imagem 496x263);
   - o `Clamp` em "sp" também (título 502x32).
4. **Conclusão:** o `StatusPage` da libadwaita 1.5 não tem defeito; o que quebra é o `Clamp` (dentro ou fora do `StatusPage`) **sem DPI de texto**. Em desktops normais isso não acontece, mas ambientes sem serviço de configurações existem.
5. **Proteção na GUI:** se `gtk-xft-dpi <= 0`, o `Clamp` passa a usar pixels. Com o DPI definido, continua em "sp", que respeita a escala de texto do usuário. Conferido visualmente no Ubuntu sem DPI definido: coluna em largura normal.
6. **Glibc:** o binário da GUI compilado no Fedora 45 exige **glibc 2.39**, exatamente a do Ubuntu 24.04, sem margem (o app mínimo do E7 exigia 2.34). O AppImage oficial deve ser compilado no Ubuntu 24.04, como o CI já faz.

### Lição de método

- Um contêiner não é um desktop: além dos drivers de vídeo (O10), faltam serviços como o de configurações. Antes de culpar uma versão de biblioteca, comparar o **mesmo ambiente** com e sem a peça suspeita.

## O12 – E12: o item do Snow na bandeja

**Data:** 2026-11-27
**Ambiente:** Fedora 45, GNOME Shell 51.1 com a extensão AppIndicator (`appindicatorsupport@rgcjonas.gmail.com`), DPI 192. Programa de teste descartável com `zbus` 5.13.2 (o mesmo do projeto): um `org.kde.StatusNotifierItem` registrado no `org.kde.StatusNotifierWatcher`, com um menu `com.canonical.dbusmenu`. O programa só registrava os cliques, sem executar nada.

### Resultados

1. **Por que o ícone do Battle.net fica borrado:** com o Battle.net aberto, os únicos itens StatusNotifierItem no D-Bus eram o Discord e o CurseForge. O Battle.net usa, pelo Wine, a **bandeja antiga do X11 (XEmbed)**, que manda um bitmap pequeno, e a extensão amplia esse bitmap.
2. **Ícone por nome:** `IconName` = `io.github.ninjairocode.SnowLauncher`, com `IconThemePath` apontando para uma pasta própria.
   - Com o SVG em `hicolor/scalable/apps/` **sem `index.theme`**, a extensão mostrou **três pontinhos** (ícone não encontrado).
   - Com o SVG **solto na raiz** do `IconThemePath`, o ícone apareceu **nítido** a 192 DPI.
3. **Menu:** a extensão leu o menu logo no registro (`GetLayout`, `AboutToShow`), e os itens apareceram normais. Cada clique chegou como `Event(id, "clicked")`, logo depois do evento `closed` do menu: "Open Snow Launcher", "Open Battle.net" e "Quit test" (que encerrou o teste).
4. **Clique esquerdo:** em 9 aberturas do menu, **nenhum `Activate`** chegou. Nesta extensão, com um menu publicado, o clique esquerdo também abre o menu.

### Consequências

- O item da bandeja é feito com `zbus` (já usado no projeto), sem biblioteca de bandeja extra.
- O ícone vai como arquivo solto no `IconThemePath`, e o menu concentra todas as ações, com "Abrir Snow Launcher" em primeiro.
- O ícone colorido ficou legível. A versão simbólica continua planejada, para seguir o estilo da barra do GNOME.

### Em aberto

- KDE (StatusNotifierItem nativo) e outras versões da extensão AppIndicator.
- Quem mostra o item: o `snowd` (sempre rodando) ou a GUI em segundo plano.
- Como esconder o ícone do Battle.net (E9).
