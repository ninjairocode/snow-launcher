# Snow Launcher

> *Para o Snow: boxer branco, mancha caramelo no olho, sorriso torto, piscadinha de lado. Agora mago oficial do projeto.*

**Snow Launcher** é um aplicativo livre para Linux (GNOME) que cria e mantém uma "casinha" onde o **Battle.net oficial** roda do melhor jeito possível sob Wine/Proton: configurado certo, vigiado o tempo todo e capaz de se consertar sozinho.

**Projeto Assistido por IA**: o desenvolvimento usa o Claude (Anthropic) como assistente. Todo código é revisado, testado e commitado por um humano.

---

## Sumário

1. Problema
2. O que o Snow Launcher é e o que não é
3. Decisões já tomadas
4. Arquitetura
5. A casinha (ambiente do Battle.net)
6. Jogos e mapa de compatibilidade
7. Correções (quirks)
8. Diagnóstico e erros conhecidos
9. Qualidade e prova de erros
10. Distribuição
11. Git, commits e transparência sobre IA
12. Roadmap
13. Experimentos a validar antes de codar
14. Riscos
15. Instruções para o Claude Code
16. Referências

---

## 1 – Problema

Quem joga títulos da Blizzard (principalmente World of Warcraft) no Linux usa o Battle.net no Wine via Lutris, Steam/Proton, Faugus etc. Os problemas mais comuns:

- **Loja do jogo não abre** dentro do WoW (bug conhecido há anos no Wine).
- **Depois que o PC suspende ou hiberna, o Battle.net não reconecta**: é preciso fechar e abrir de novo.
- **"Battle.net Agent went to sleep" (BLZBNTBNA00000005)** e travamentos após atualizações do app.
- **Interface lenta e travada**: o Battle.net é um navegador Chromium embutido (CEF) e roda mal com configuração errada.
- **Atualizações da Blizzard quebram tudo periodicamente**, e o usuário precisa caçar a versão de Wine que funciona em fóruns.

As ferramentas atuais (Lutris, Faugus, Heroic) são genéricas. Nenhuma é focada em Blizzard, vigia os processos ou corrige esses erros automaticamente.

## 2 – O que o Snow Launcher é e o que não é

### É

- Um **gerenciador do ambiente** (prefixo Wine + runtime Proton) dedicado ao Battle.net.
- Um **vigia** em segundo plano que detecta suspender/acordar, erros e travamentos, e reage.
- Um **catálogo de correções** declarativas, atualizado remotamente pela comunidade.
- Uma **interface GNOME nativa** para abrir jogos, ver status e ajustar resolução, escala e gamescope.

### Não é, e nunca será

- **Não é um cliente Battle.net alternativo.** Não implementa nem imita os protocolos da Blizzard (login, download, patch). Isso protege as contas dos usuários e o projeto. Quem quiser esse caminho tem o ArcaneLauncher.
- **Não injeta código nos jogos**, não lê nem altera memória dos jogos e não automatiza gameplay.
- **Não toca em credenciais.** O login é sempre feito na janela oficial do Battle.net.
- **Não contorna anti-cheat.** Jogos bloqueados por anti-cheat em nível de kernel ficam marcados como "fora de alcance".
- **Ferramentas de terceiros só por escolha do usuário.** MangoHud, GameMode e gamescope são programas que o próprio usuário instala. O Snow só os ativa pelos mecanismos padrão de cada um (variáveis de ambiente ou wrapper, como o Lutris e a Steam fazem), sempre **desligados por padrão**. O Snow em si nunca injeta código nem lê memória dos jogos.
- **Não usa marcas da Blizzard** no nome, no ícone ou na identidade visual. O README deve trazer um aviso de que o projeto não é afiliado à Blizzard Entertainment.

### Promessa do produto

> "O Battle.net configurado do jeito certo, que se conserta sozinho e quase não te atrapalha."

O Snow Launcher **não** promete que o Battle.net roda mais rápido que num Lutris perfeitamente configurado (o motor é o mesmo). O ganho vem de configuração correta por padrão, recuperação automática e de usar menos a interface lenta.

## 3 – Decisões já tomadas

| Tema | Decisão |
|---|---|
| Nome | **Snow Launcher**: homenagem ao Snow, o boxer branco do mantenedor, com a mancha caramelo no olho. O ícone é ele, de chapéu e cajado de mago, com a piscadinha que era a marca dele |
| Linguagem | **Rust** |
| Interface | **GTK4 + libadwaita** via **Relm4** (visual nativo GNOME) |
| Runtime Windows | **umu-launcher + GE-Proton** |
| Distribuição inicial | **AppImage** (GitHub Releases) |
| Abordagem | Envolver o Battle.net oficial (nunca substituí-lo) |
| Janela do Battle.net | Não é embutida. Roda escondida, e a casinha mostra o progresso; a janela só aparece para login, loja/amigos ou erro |
| Onde ficam os jogos | Na **biblioteca**, fora do prefixo, mapeada como `G:`. Importar **copia por reflink** quando o sistema de arquivos permite (btrfs, XFS: instantâneo, sem espaço extra, o outro launcher continua funcionando) e, senão, move |
| Transparência | "Projeto Assistido por IA" no README + trailer `Assisted-by:` |
| Commits | Feitos manualmente pelo mantenedor, com orientação do Claude |
| Licença | **GPL-3.0-or-later**, a mesma família do umu-launcher, comum no ecossistema GNOME (arquivo `LICENSE`) |
| App ID | `io.github.ninjairocode.SnowLauncher` |
| Base mínima suportada | **Ubuntu 24.04 LTS**: GTK 4.14 e libadwaita 1.5 (cobre Fedora 40+, Arch, Debian 13). Libera o `adw::ToolbarView` (1.4+); deixa de fora o Ubuntu 22.04 (GTK 4.6, libadwaita 1.1). Toda tela da GUI é testada nessa base (E7) |
| Versão mínima do Rust (MSRV) | **1.85** (edition 2024), verificada no CI. Subir quando uma dependência exigir |
| Configuração | `~/.config/snow-launcher/config.toml`, `schema = 1`. Chaves: `prefix` e `[sleep] allow_suspend_while_playing`. Erros no arquivo viram avisos, nunca param o daemon |
| Suspensão com jogo aberto | **Bloqueada por padrão** (portal Inhibit, só a flag *Suspend*); liberável no `config.toml`. Ver seção 5 |
| Interface D-Bus | Barramento de **sessão**, só **propriedades de leitura** por enquanto (seção 4) |
| Notificações | `org.freedesktop.Notifications` direto (funciona fora de sandbox: AppImage, RPM). O Flatpak vai precisar do portal de notificações |
| Fluxo de trabalho | Tudo por PR, com CI verde; merge por *Rebase and merge*; `main` protegido (ruleset `protect-main`) |

**Pendência:** verificar se o nome "Snow Launcher" já é usado por algum app Linux, no GitHub ou no Flathub antes do primeiro release.

**Nomes técnicos:** binários `snow-launcher` (GUI), `snowd` (daemon) e `snowctl` (CLI). Crates com prefixo `snowlauncher-` para não confundir com o crate `snow` do crates.io, que é uma biblioteca de criptografia sem relação com o projeto.

## 4 – Arquitetura

### Workspace Cargo

```
snow-launcher/
├── Cargo.toml                 # workspace
├── crates/
│   ├── snowlauncher-core/           # domínio: tipos, config, catálogo de erros, regras, nomes do D-Bus. Sem IO direto.
│   ├── snowlauncher-wine/           # prefixo, processos (/proc), logs do Battle.net, config.toml em disco, instância única
│   ├── snowlauncher-daemon/         # binário `snowd`: vigia, sleep-hook, inibição de suspensão, conexões, D-Bus
│   ├── snowlauncher-cli/            # binário `snowctl`: status, doctor, service (depois: launch, quirks)
│   └── snowlauncher-gui/            # binário `snow-launcher`: Relm4 + libadwaita (v0.3, ainda não existe)
├── quirks/                    # correções embutidas (fallback offline)
├── data/                      # ícones, unidade systemd (depois: .desktop, metainfo.xml)
├── docs/
└── tests/                     # testes de integração
```

### Comunicação entre as partes

- O `snowd` expõe uma interface **D-Bus na sessão do usuário** (`io.github.ninjairocode.SnowLauncher1`), usando `zbus`. Hoje ela tem só propriedades de leitura, com `PropertiesChanged` a cada mudança: `Version`, `Prefix`, `RunningGames`, `SuspendInhibited` e `GameConnection` (`no-game`, `ok`, `network-down`, `lost`). Os nomes ficam em `snowlauncher_core::dbus`.
- A GUI e a CLI são **clientes** desse D-Bus. Não duplicam lógica.
- O `snowd` roda como **serviço do usuário** (`systemd --user`), instalado por `snowctl service install` (com `--dry-run`, backup e `uninstall`). Sobe com a sessão gráfica e desiste depois de 3 falhas em 10 minutos. Só pode haver um `snowd` por usuário (flock em `$XDG_RUNTIME_DIR/snow-launcher.lock`).

### Fluxo principal

```
[logind] --PrepareForSleep--> [snowd/sleep-hook] --> [supervisor] --> reinicia Battle.net/Agent
[logs do Battle.net] ------> [doctor] --> identifica erro --> [quirks] --> aplica correção
[GUI/CLI] --D-Bus--> [snowd] --> umu-run --> Battle.net.exe --exec="launch <jogo>"
```

### Bibliotecas Rust previstas

| Função | Crate |
|---|---|
| Async | `tokio` |
| D-Bus (logind, interface própria) | `zbus` |
| Processos | leitura direta de `/proc` (ou `procfs`), `nix` para sinais |
| Config e quirks | `serde`, `toml` |
| Erros | `thiserror` (bibliotecas), `anyhow` (binários) |
| Logs | `tracing`, `tracing-subscriber`, `tracing-journald` |
| Diretórios XDG | `directories` |
| HTTP (runtime e quirks) | `reqwest` com rustls |
| Verificação de integridade | `sha2`, assinatura com `minisign-verify` |
| GUI | `relm4`, `libadwaita` |
| Notificações | portal via `ashpd` |
| Lock de instância única | `fs4` (flock) |

### Onde ficam os arquivos (XDG)

| Caminho | Conteúdo |
|---|---|
| `~/.config/snow-launcher/config.toml` | Configuração do usuário |
| `~/.local/share/snow-launcher/prefixes/<nome>/` | Prefixos Wine gerenciados |
| `~/.local/share/snow-launcher/runtimes/` | Protons colocados ali pelo usuário (o Snow não baixa; ver 12.2) |
| `~/.local/share/snow-launcher/quirks/` | Cache do repositório de correções |
| `~/.local/state/snow-launcher/` | Logs, histórico de correções aplicadas, backups |
| `~/.cache/snow-launcher/` | Downloads temporários |
| `~/Games/SnowLauncher/` | **Biblioteca de jogos** (padrão, configurável). Fica fora dos prefixos e aparece como `G:` na casinha |

## 5 – A casinha (ambiente do Battle.net)

A casinha é um **prefixo Wine controlado pelo Snow Launcher**. Ela inclui:

- **Runtime fixado**: o Proton gravado no registro da casinha não muda sozinho, e trocar faz backup do prefixo, com **volta segura** (`restore-prefix`) se algo quebrar. O usuário pode escolher outra versão (ver "Escolha do Proton e integração com o ProtonPlus"); o Snow não baixa Protons (ver 12.2).
- **Ajustes padrão do prefixo**:
  - Aceleração por hardware do navegador do Battle.net **desligada**.
  - **Windows 11** (`winecfg -v win11`): forte indício de que a loja do jogo depende disso (`docs/experimentos.md`, O6). Falta o teste inverso para confirmar.
  - Pasta de jogos acessada por **letra de unidade mapeada**, não por link simbólico (correção conhecida da loja).
  - **NTSYNC** quando `/dev/ntsync` existir e for acessível. Caso contrário, fsync/esync, e a GUI informa qual está ativo.
  - **Escala da interface (DPI) automática**, para o instalador e o Battle.net virem legíveis por padrão (ver "Escala da interface").
- **Vigia**:
  - Escuta `PrepareForSleep` do logind. Ao acordar, aguarda a rede voltar (NetworkManager) e reinicia **Battle.net e Agent** de forma limpa.
  - **Nunca encerra um jogo em execução.** Se o WoW estiver aberto ao acordar, reinicia só Battle.net e Agent, ou avisa o usuário por notificação. O comportamento é configurável.
  - **Sem suspensão automática com jogo aberto (padrão).** Enquanto um jogo estiver rodando, o `snowd` pede ao sistema para não suspender por inatividade (portal `org.freedesktop.portal.Inhibit`, só a flag *Suspend*). A tela ainda pode apagar, e a suspensão manual pelo menu continua funcionando.
    - **Motivo:** durante a suspensão o servidor encerra a sessão do jogo. Ao acordar, o jogo fica com uma conexão TCP "meio aberta" e pode levar ~15 min para perceber (`tcp_retries2 = 15`). Não há como forçar a desconexão sem root nem sem mexer no jogo.
    - **Opção do usuário:** `[sleep] allow_suspend_while_playing = true` no `config.toml` libera a suspensão automática mesmo com jogo aberto.
  - **Aviso quando a conexão do jogo morre.** Enquanto um jogo roda, o `snowd` olha as conexões TCP dele a cada 5 s (`/proc/<pid>/net/tcp`, sem root). Depois de uma queda de rede longa o bastante (suspensão, cabo, roteador), o servidor encerra a sessão e o jogo fica com uma conexão "meio aberta": dá até para andar com o personagem, mas nada chega ao servidor. Uma conexão estabelecida que retransmite sem resposta (6+ retransmissões) em duas amostras seguidas, **com a rede local online**, é considerada morta, e o Snow mostra uma notificação (uma por problema): o jogo perdeu a conexão e precisa ser fechado e aberto de novo. Enquanto a rede local está fora, retransmitir é normal e não conta: a conexão pode se recuperar quando a rede voltar.
  - **Jogos são reconhecidos por uma lista fechada de executáveis** (que depois vem do repositório de quirks), nunca por "qualquer `.exe` desconhecido". Um falso positivo seguraria a suspensão sem motivo.
  - Detecta Battle.net travado (janela sem resposta, processo zumbi, loop do Agent) e oferece ou aplica a correção.
- **Battle.net discreto**: o Snow Launcher abre os jogos com `Battle.net.exe --exec="launch <código>"`, e a janela do Battle.net fica escondida até ser necessária (ver "Inicialização e a janela do Battle.net").
  - **Atenção:** o GNOME não tem bandeja do sistema nativa. Use o portal **Background** (aparece em "Apps em segundo plano") e notificações, sem depender de AppIndicator.

### Inicialização e a janela do Battle.net

**Decisão: o Battle.net não é embutido na janela do Snow Launcher.**
- Embutir a janela de outro programa exigiria XEmbed (só existe no X11, foi removido do GTK4 e não existe no Wayland) ou um compositor Wayland próprio dentro do app (complexo e frágil demais).
- O "desktop virtual" do Wine também foi descartado: ele prende os jogos na mesma janela, sem tela cheia de verdade e sem uso correto de vários monitores.

**Como funciona então:** o Battle.net trabalha escondido e a casinha mostra o andamento na própria janela do Snow Launcher:

1. "Preparando o ambiente…" (runtime, prefixo, correções pendentes)
2. "O Battle.net está iniciando…"
3. "Pronto", e o jogo abre direto se o usuário clicou em "Jogar"

- Tela de progresso com o Snow animado (chapéu girando, floquinhos caindo).
- Botão "Cancelar" em todas as etapas, com encerramento limpo dos processos.

**A janela do Battle.net só aparece quando:**
- **é preciso fazer login:** se o Battle.net não ficar pronto em N segundos, o Snow mostra "Entre na sua conta no Battle.net" com o botão "Mostrar Battle.net";
- **o usuário clica em "Abrir Battle.net"** (loja, amigos, configurações);
- **acontece um erro**, junto com o diagnóstico e a correção sugerida.

**Limite do Wayland:** um app não consegue ler nem controlar janelas de outros programas. O estado do Battle.net (iniciando, aguardando login, pronto, travado) é deduzido **pelos processos e pelos logs dele**, nunca "olhando" a janela. Os padrões de log usados para isso ficam no repositório de quirks, para poder ser atualizados.

### Casinha e biblioteca

**Decisão: os jogos ficam fora do prefixo.** O prefixo (a casinha) é descartável. A biblioteca é o que importa.

```
~/.local/share/snow-launcher/prefixes/battlenet/   ← casinha (pode ser apagada e recriada)
└── drive_c/
    ├── Program Files (x86)/Battle.net/            ← só o app Battle.net
    └── ProgramData/Battle.net/

~/Games/SnowLauncher/                              ← biblioteca (padrão, configurável)
├── World of Warcraft/
└── Diablo IV/
```

- Dentro da casinha, a biblioteca aparece como a unidade **`G:`**. Para o Battle.net, o WoW está em `G:\World of Warcraft`. É a mesma técnica da correção da loja do jogo.
- **Vale para tudo:** instalações novas também vão para `G:\`, não só as importadas.
- **Vantagem:** a casinha pode ser recriada, resetada ou trocar de Proton sem perder nenhum jogo nem baixar tudo de novo.
- A pasta da biblioteca é escolhida na primeira execução (padrão `~/Games/SnowLauncher/`, sem espaços no nome, para evitar problemas com caminhos). O app avisa se o disco escolhido tiver pouco espaço.
- **Trocar a pasta da biblioteca depois:** usa o mesmo processo de mover da importação (rename no mesmo disco, ou copiar e verificar entre discos).

### Escolha do Proton e integração com o ProtonPlus

O usuário pode usar o Proton que quiser, mas o Snow continua recomendando a versão testada.

**De onde vêm as versões:**
- **Pasta própria do Snow** (`~/.local/share/snow-launcher/runtimes/`): versão recomendada, baixada e verificada pelo próprio app. Funciona sem nenhuma ferramenta extra.
- **Pastas de compatibilidade da Steam** (`compatibilitytools.d`, nas instalações nativa e Flatpak da Steam): onde o ProtonPlus e o ProtonUp-Qt costumam instalar GE-Proton, Proton-CachyOS etc. O Snow só lê essas pastas, nunca altera nem apaga nada nelas.
- O umu-launcher aceita qualquer uma pela variável `PROTONPATH`.

**Integração com o ProtonPlus:**
- O ProtonPlus é **opcional**, nunca uma dependência.
- Abrir uma issue ou PR no ProtonPlus para incluir o **Snow Launcher como launcher suportado**, apontando para a pasta de runtimes do Snow. Assim quem já usa o ProtonPlus instala versões direto para o Snow.
- Se o ProtonPlus estiver instalado, o Snow mostra o atalho "Gerenciar versões no ProtonPlus".

**Regras de segurança:**
- A lista mostra **"Recomendada (testada com o Battle.net)"** em destaque. As outras aparecem como **"Não testada pelo Snow"**, com aviso ao selecionar.
- **Backup do prefixo antes de trocar de versão.** Um Proton mais novo atualiza o prefixo, e voltar para uma versão mais velha pode quebrar sem esse backup. Trocar de volta oferece restaurar o backup.
  - **Feito no `snowctl house runtime set`** (O15): cópia por reflink (`cp -a --reflink=auto`, instantânea no Btrfs/XFS) em `~/.local/share/snow-launcher/backups/prefix-<hora>`, mais o backup do registro. Só com a casinha vazia. Restaurar: `snowctl house restore-prefix [backup]` (sem argumento lista os backups), que guarda o prefixo atual em `backups/prefix-replaced-<hora>` antes de pôr a cópia do backup no lugar; testado na casinha real (0,22 s, e o Battle.net abriu e logou no prefixo restaurado).
- Se a versão escolhida for removida do sistema (por exemplo, apagada no ProtonPlus), o Snow avisa e oferece voltar para a recomendada, em vez de falhar ao abrir.
- A versão recomendada vem do repositório de quirks, para poder mudar sem lançar versão nova do app.

### Instalação do Battle.net na casinha

O Snow Launcher oferece três caminhos. O assistente de primeira execução pergunta qual usar, começando pela importação, se encontrar algo.

**1 – Baixar pelo navegador e a cópia verificada (padrão)**
- O Snow **não baixa** o instalador: ele **abre a página oficial de download no navegador** (`snowctl house installer download`), e o próprio usuário baixa. Assim o Snow não conversa com os servidores da Blizzard (regra do `CLAUDE.md`).
- **Nunca** embutir o instalador no app, no repositório ou hospedar cópia (o arquivo é da Blizzard; redistribuir é problema de direitos autorais). Decidido com o mantenedor em 2026-11-28, depois de considerar guardar o `.exe` no repositório.
- Na instalação, o Snow procura o instalador **em `~/Downloads`** (o mais novo primeiro) e confere a assinatura de cada um.
- **A última cópia que passou na verificação fica guardada** em `~/.cache/snow-launcher/installer/` e é o **padrão** nas próximas instalações (recriar a casinha, por exemplo), sem baixar de novo. Um download mais novo e válido a substitui.
- A versão do instalador quase não importa: depois de instalado, o Battle.net se atualiza sozinho.
- `snowctl house installer status` mostra a cópia guardada e o que há em Downloads, já verificados.
- **Na janela:** com a casinha sem Battle.net, os botões "Install Battle.net" (usa essa mesma escolha verificada e mostra o progresso com as luzinhas) e "Open download page". Testado numa casinha de teste: o instalador rodou, o Battle.net abriu e a cópia verificada foi guardada.

**2 – Arquivo local**
- `--installer PATH` (na janela, "Usar um arquivo meu"), também verificado. Se o arquivo indicado falhar, o Snow **não** troca por outro: para e explica.

**3 – Importar instalação existente (o mais importante para quem já joga)**

Importar sempre traz o jogo **para a biblioteca do Snow** (ver "Casinha e biblioteca"). Não existe a opção de usar o jogo no lugar, para o jogo nunca depender da pasta de outro launcher.

**Copiar por reflink, quando der (preferido):** em btrfs e XFS no mesmo sistema de arquivos, `cp --reflink=always` faz uma cópia **independente e instantânea, sem gastar espaço**: as duas pastas compartilham os blocos até um lado mudar um arquivo. O jogo passa a existir na casinha **e** continua funcionando no outro launcher. Validado com o WoW do mantenedor: 49,6 GB e 4.942 arquivos em 1 s, ocupando 14 MB (O9). Sem reflink, vale o fluxo de mover abaixo.

1. Detectar prefixos do Lutris, da Steam (compatdata) e do Faugus, e as pastas de jogos Blizzard dentro deles (`World of Warcraft`, `Diablo IV` etc.).
2. Mostrar o que foi encontrado, com tamanho, e explicar antes de confirmar: **"Depois de mover, este jogo deixa de funcionar no Lutris."**
3. Conferir antes de mover:
   - o Battle.net e o jogo **não podem estar abertos** no outro launcher (processos com aquele `WINEPREFIX`);
   - se a biblioteca está em **outro disco**, verificar se há espaço livre suficiente.
4. Trazer:
   - **Reflink disponível:** copiar por reflink e conferir arquivo a arquivo (caminho e tamanho); o original fica intacto.
   - **Mesmo disco, sem reflink:** `rename` da pasta. É instantâneo e atômico, não copia nada.
   - **Outro disco:** copiar → verificar (quantidade de arquivos e tamanhos) → só então **oferecer** apagar o original. Nunca apagar antes de confirmar a cópia. Se a cópia for interrompida, a pasta parcial é descartada e o original continua intacto.
5. Abrir a casinha e guiar o usuário no **"Localizar jogo"** do Battle.net, apontando para `G:\<nome do jogo>`. O Battle.net verifica os arquivos e baixa só o que faltar. O Battle.net não acha sozinho um jogo copiado (O6, O17), e o Snow não escreve nos dados dele para forçar isso.

**Feito no `snowctl import`** (O17): sem argumento, lista os jogos dos outros launchers (Lutris, Steam, `~/Games`); com a pasta, copia para a biblioteca da casinha numa pasta temporária, por reflink quando o disco permite (senão, cópia normal depois de conferir o espaço), confere arquivo a arquivo e só então dá o nome final. O original nunca é apagado nem alterado (o "oferecer apagar o original" do passo 4 fica para depois). Recusa se o jogo já está na biblioteca ou se algo roda no prefixo de origem. **Na janela:** "Import a game…" ao lado de "Games" abre um diálogo com os jogos encontrados (tamanho e origem, um "Import" por jogo) e "Choose a folder…" (seletor de pastas do sistema); depois de importar, mostra o caminho do "Localizar jogo" e atualiza a lista de jogos.
6. Registrar no histórico de onde o jogo veio, para facilitar o suporte.

O app Battle.net e os dados do outro launcher **não são importados**: a casinha instala o seu próprio Battle.net. Só os jogos mudam de lugar.

**Verificação do instalador (caminhos 1 e 2)**
- A Blizzard não publica checksum, mas o `.exe` tem **assinatura digital Authenticode**. Antes de executar, conferir se a assinatura é válida e emitida para **Blizzard Entertainment**. Se não for, recusar e explicar o motivo.
- Download em arquivo temporário. Só é usado depois de completo e verificado.

**Instalação vigiada**
- O instalador do Battle.net costuma travar ou ficar em 0% no Wine (o script do Lutris, por exemplo, pede para fechar o app na primeira tela de login).
- O Snow Launcher trata a instalação como um processo vigiado: inicia o instalador, detecta quando terminou ou travou (arquivos criados, processo ocioso, timeout), encerra de forma limpa e reabre o Battle.net já instalado.
- Os detalhes dependem do experimento E6.

### Escala da interface (DPI)

O Wine desenha tudo a **96 DPI**, sem acompanhar a escala do desktop. Num monitor 1440p com escala de 125%, o instalador e o Battle.net ficam pequenos demais. **Ninguém deveria precisar ajustar isso à mão.**

**Validado no E8 (O7):** com o GNOME a 125% no Wayland, o desktop informa `Xft.dpi = 192` às aplicações X, e o Wine com **192 DPI ficou do tamanho certo**. A ideia central do plano: o Snow não calcula a escala sozinho, ele **pergunta ao desktop**, que já sabe como os aplicativos X são desenhados naquela tela.

#### Como o DPI é escolhido (função pura no `core`, com testes de tabela)

1. **Opção do usuário**, se houver (`ui_scale` em porcentagem): DPI = 96 × porcentagem.
2. Senão, o **`Xft.dpi`** que o desktop publica para o X. O GNOME e o KDE ajustam esse valor conforme a escala e o modo de desenho dos aplicativos X.
3. Senão, **96 × a escala do monitor principal**, se for possível descobrir.
4. Senão, **96**.

O resultado é **limitado a 96–288** (100% a 300%) e **arredondado para múltiplos de 24** (degraus de 25%), que é o que o Windows usa e evita texto borrado. O cálculo fica registrado no log, com a fonte usada (opção, `Xft.dpi`, monitor ou padrão).

#### Casos que o plano precisa cobrir

| Ambiente | O que o desktop informa | Esperado |
|---|---|---|
| GNOME Wayland, escala fracionária (125%, 150%) | `Xft.dpi` = 192 (desenha em 2× e reduz) | 192 (**validado a 125%**) |
| GNOME Wayland, 100% ou 200% | `Xft.dpi` = 96 ou 192 | igual ao `Xft.dpi` |
| KDE Wayland, "aplicativos X escalam sozinhos" | `Xft.dpi` = 96 × escala | igual ao `Xft.dpi` |
| KDE Wayland, "o sistema escala os aplicativos X" | `Xft.dpi` = 96; o compositor amplia (fica borrado) | 96. Ampliar também no Wine dobraria o tamanho |
| Sessão X11 (sem Wayland) | `Xft.dpi` das configurações do desktop | igual ao `Xft.dpi` |
| Sem `Xft.dpi` publicado | — | 96 × escala do monitor, ou 96 |
| Vários monitores com escalas diferentes | Um `Xft.dpi` só, global | Limitação do Wine: **um DPI por prefixo**. Usar o do desktop e permitir ajuste manual |
| Driver Wayland nativo do Wine (futuro, fora do padrão do Proton) | O próprio Wine trata a escala | Não aplicar DPI extra; revisar quando o Proton adotar |

Cada linha vira um caso de teste da função pura. As linhas não validadas na prática entram no E8 como "a confirmar com usuários" (pedir relatos com o `doctor`).

#### Quando aplicar

- **Na criação da casinha, antes de rodar o instalador**, para o instalador já vir legível.
- **A cada inicialização da casinha, antes de abrir o Battle.net:** se estiver em `auto` e o DPI calculado mudou (outro monitor, outra escala), reaplicar. É só um valor no registro, gravado com o Battle.net fechado (o Wine só lê o DPI ao iniciar). Com uma porcentagem fixa escolhida pelo usuário, nunca mexer.
- O valor aplicado fica no estado do Snow, para comparar depois. O `doctor` mostra o DPI aplicado, a fonte usada e uma eventual diferença.

#### Como aplicar

- As **três chaves `LogPixels`** que o Lutris também usa: `HKCU\Control Panel\Desktop`, `HKCU\Software\Wine\Fonts` e `HKLM\System\CurrentControlSet\Hardware Profiles\Current\Software\Fonts`.
- Por uma ação declarativa de quirk (`set_registry`), com backup, registro no histórico e reversível, como todas as ações da seção 7.
- **Junto com `QT_ENABLE_HIGHDPI_SCALING=0` em tudo o que roda na casinha (obrigatório).** O instalador e a janela de login do Battle.net são Qt 5.15.1. Acima de 100% de DPI, o Qt liga um modo próprio de escala para alta densidade que, no Wine, **trava o instalador** e **quebra a etapa da senha do login** (violação de acesso no `Qt5Core.dll`). Com esse modo desligado, o Qt usa o DPI do Wine para o tamanho de textos e elementos: tudo funciona e fica legível a 192 (E11, O9). A janela principal do Battle.net é desenhada pelo CEF e não tem o problema, por isso o E8 (com sessão salva) não o mostrou.

#### Opção do usuário

```toml
[display]
ui_scale = "auto"   # ou uma porcentagem: 100, 125, 150, 200...
```

Na GUI (v0.3): "Escala da interface: Automática (200%)", com as opções de porcentagem e um aviso de que o Battle.net precisa reabrir para aplicar.

#### A verificar ainda

- Se o DPI muda algo **dentro dos jogos** (a interface do WoW tem escala própria; a 144 no Lutris não houve problema).
- Os casos da tabela não validados, com relatos de usuários.

### MangoHud e GameMode

Opções para quem usa essas ferramentas, como o mantenedor já faz no Lutris (`gamemoderun mangohud`). **Desligadas por padrão** (ver seção 2):

```toml
[performance]
mangohud = false   # overlay de FPS/uso nos jogos (o MangoHud do usuário)
gamemode = false   # Feral GameMode enquanto um jogo roda
```

- **MangoHud** via `MANGOHUD=1` no ambiente com que o Snow inicia o Battle.net: os jogos abertos por ele herdam (E15, O16) e usam a configuração do próprio usuário. O Battle.net não carrega o overlay, então não precisa de `blacklist`. Vale na próxima vez que o Battle.net abrir; não dá para ligar por jogo. **Feito.**
- **GameMode** pelo `snowd`: quando um jogo abre, ele registra **só o processo do jogo** no GameMode pelo D-Bus (`RegisterGameByPID`), e o GameMode o solta quando o jogo fecha. Sem `gamemoderun` nem `LD_PRELOAD`, que no contêiner do umu seriam um problema, e sem modo jogo no Battle.net. O `snowd` acompanha a opção no `config.toml` enquanto roda: desligar solta o jogo na hora. **Feito.**
- **Na janela:** Preferências → Performance, com uma chave para cada. Em Preferências → House ficam a escolha do Proton (com backup do prefixo; recusada com a casinha em uso) e a chave de esconder o ícone do Battle.net (E14). A chave grava só aquela linha do `config.toml` (comentários e o resto do arquivo ficam como estão), com backup antes; fica desabilitada se a ferramenta não estiver instalada.
- Se a opção estiver ligada e a ferramenta não estiver instalada, o Snow avisa (`doctor`, barra de status) em vez de falhar.

### Um prefixo por jogo ou um para todos?

Quem abre o jogo é o Battle.net, e o jogo herda o ambiente dele. Ajustes valem para **todos os jogos do mesmo prefixo**. Estratégia:

- **Padrão:** um prefixo compartilhado.
- **Perfis isolados:** quando um jogo precisar de ambiente incompatível com os outros, o Snow Launcher cria um prefixo separado (com seu próprio Battle.net) e reaproveita os arquivos do jogo por caminho mapeado.
- Decidir os detalhes depois do experimento E5.

## 6 – Jogos e mapa de compatibilidade

Cada jogo tem um status, com explicação visível ao usuário:

| Status | Significado | Ação do Snow Launcher |
|---|---|---|
| ✅ Funciona | Roda direto | Nenhuma |
| 🔧 Funciona com ajustes | Precisa de quirks | Aplica automaticamente |
| 🧪 Problema no Wine/Proton | Bug na camada de compatibilidade | Acompanha. Adota a versão do Proton que corrigir |
| ⛔ Fora de alcance | Anti-cheat em nível de kernel ou bloqueio da empresa | Explica o motivo. Não promete solução |

- **O status vem do repositório remoto de quirks** (seção 7), não fica fixo no código.
- **Avaliar reaproveitar o umu-database e o umu-protonfixes** para identificar jogos e correções já conhecidas.
- **Por jogo:** gamescope (resolução, tela cheia, FSR, limite de FPS), argumentos extras e variáveis de ambiente, sempre limitados pelo que o prefixo permite (ver seção 5).

### Tela de jogos instalados

A tela principal da casinha tem:
- **Barra de status no topo:** Battle.net conectado, runtime fixado, NTSYNC ativo, vigia de descanso ligado. Cada item é clicável e abre o diagnóstico daquele ponto.
- **Cards dos jogos instalados:** nome, edição (Retail/Classic), tamanho, status de compatibilidade e o botão "Jogar". O botão vira "Rodando" enquanto o processo do jogo existir.
- **Menu "⋯" de cada jogo:** configurações do jogo (gamescope, resolução, FSR, limite de FPS), abrir pasta do jogo, verificar no Battle.net (abre a página do jogo, que tem a verificação oficial) e ver correções aplicadas.
- **Rodapé "Tem jogos no Lutris ou na Steam? Importar".**
- **Botão "Abrir Battle.net"** no cabeçalho.

**Como detectar os jogos (somente leitura):**
1. **Registro do prefixo** (`system.reg`, arquivo de texto): entradas de desinstalação criadas pelo Battle.net, com nome e pasta de cada jogo. É a fonte principal, porque segue o padrão do Windows e é estável.
2. **`.build.info` na pasta do jogo:** produto e versão instalada (identifica Retail, Classic etc.).
3. **`product.db` do Battle.net:** só como complemento, porque é formato interno e pode mudar. **Nunca escrever nele**, porque corromper esse arquivo quebra o Battle.net.
- O tamanho é a soma da pasta, calculada em segundo plano e guardada em cache.
- A lista é atualizada quando o Battle.net instala ou remove algo (observar as pastas com inotify) e ao abrir o app.
- A CLI expõe o mesmo: `snowctl games` e `snowctl launch <jogo>`.

**Como abrir:**
- `Battle.net.exe --exec="launch <código>"`. **Os códigos ficam no repositório de quirks**, não no código do app.
- Se o Battle.net estiver fechado, o Snow inicia, espera ficar pronto (ver seção 5) e depois abre o jogo. Atualizações pendentes são tratadas pelo próprio Battle.net.

**Visual:**
- **Sem artes ou logos oficiais da Blizzard no projeto.** Nada disso entra no repositório, no AppImage ou no site.
- **Ícones dos jogos tirados da instalação do próprio usuário.** O `Wow.exe` e o `Battle.net.exe` trazem o próprio ícone embutido, em até 256×256 (conferido). O Snow lê esse ícone do executável **na máquina do usuário**, guarda uma cópia em `~/.cache/snow-launcher/icons/` e mostra nos cards dos jogos, na barra de status e nas notificações (por exemplo, o aviso de conexão perdida com o ícone do jogo). É o mesmo que um gerenciador de arquivos faz ao mostrar o ícone de um `.exe`: nada é distribuído pelo Snow. Sem ícone legível, usar um quadrado com a inicial no estilo do Snow.
- No libadwaita: `GtkFlowBox` de cards numa página simples. O Bottles é uma boa referência visual.

### Bandeja: só o Snow

O ícone que o Battle.net põe na bandeja fica **pequeno e borrado**: o Battle.net nem é um item StatusNotifierItem. Pelo Wine, ele usa a bandeja antiga do X11 (XEmbed), que manda um bitmap pequeno (16 ou 32 px), e a bandeja amplia (E12, O12). Em vez de tentar consertar isso, a bandeja passa a ter **um item só, o do Snow**, e o Battle.net sai de lá. O Snow é o ponto de entrada para o Battle.net e para os jogos.

- **Ícone:** a versão **simbólica** (monocromática) do ícone do Snow, no estilo dos outros ícones da barra do GNOME. A casinha colorida perde a forma em 16–22 px. Por isso o ícone simbólico sobe da v0.6 para a **v0.3**.
- **Menu:**

  | Item | O que faz |
  |---|---|
  | *Linha de estado* (não clicável) | Ex.: "World of Warcraft rodando · suspensão bloqueada", lido do D-Bus do `snowd` |
  | Abrir Battle.net | Traz a janela do Battle.net, ou abre se estiver fechado |
  | Jogar ▸ | Um item por jogo instalado, com o ícone dele, abrindo direto (`--exec="launch …"`) |
  | Abrir Snow Launcher | A janela principal |
  | Diagnóstico | O mesmo do `snowctl doctor`, em janela |
  | Sair do Battle.net | Fecha o Battle.net e o Agent de forma limpa, **nunca** um jogo aberto. Só aparece com o Battle.net aberto |
  | Sair do Snow Launcher | Encerra o `snowd` (o ícone some) e a janela. O Battle.net e os jogos continuam; sem o Snow, ficam sem as proteções (suspensão, aviso de conexão). Abrir a janela liga o Snow de novo |

- **Esconder o item do Battle.net:** **feito** (E14, O14). A política do Windows `NoTrayItemsDisplay` (em `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer`), que o `explorer.exe` do Wine respeita, tira da bandeja os ícones dos programas da casinha. A condição de segurança vem do E9: rodar o `Battle.net.exe` de novo traz a janela de volta, e é isso que o "Abrir Battle.net" do Snow faz. Comando: `snowctl house battlenet-icon hide|show|status`, com `--dry-run`, backup da chave e conferência por `reg query`. **Padrão em toda casinha nova:** o `house create` já esconde o ícone (passo conferido por `reg query`); quem quiser o ícone de volta usa as Preferências ou o `show`.
- **Quem mostra o item:** o `snowd` (decidido na v0.3).
- **Início com a sessão:** "Start Snow at login" nas Preferências (`systemctl --user enable/disable snowd.service`), ligado por padrão. Desligado, nada do Snow roda até a janela ser aberta: **a janela inicia o `snowd`** quando ele está instalado e parado (também depois de "Sair do Snow Launcher").
- **Abrir o Battle.net quando o Snow inicia:** "Open Battle.net when Snow starts" nas Preferências (`[startup] open_battlenet`, desligado por padrão). O `snowd` abre o Battle.net **uma vez por sessão** ao iniciar (no login, ou quando a janela o inicia), se a casinha está pronta e o Battle.net fechado. A marca "já abri nesta sessão" fica em `$XDG_RUNTIME_DIR`, limpo no logout, então um `snowd` reiniciado no meio da sessão não reabre o Battle.net. Testado: abriu no "login" e não reabriu no reinício.
- Sem bandeja (GNOME sem a extensão AppIndicator), valem o portal Background e as notificações.
- Usa StatusNotifierItem pelo D-Bus: nativo no KDE, e no GNOME com a extensão AppIndicator. **Validado no E12** (O12), com `zbus`, sem biblioteca extra:
  - o ícone vai **por nome** (`IconName`) com um `IconThemePath` próprio, e o SVG fica **solto na raiz** desse caminho. Numa estrutura `hicolor/…` sem `index.theme`, a extensão mostra três pontinhos. Assim o ícone é vetor e fica nítido a 192 DPI;
  - o menu é um `com.canonical.dbusmenu`, e os cliques chegam como evento `clicked`;
  - na extensão AppIndicator, o **clique esquerdo também abre o menu** (nenhum `Activate` chegou). Então tudo passa pelo menu, com "Abrir Snow Launcher" como primeiro item.

### Controle do Battle.net

A bandeja, a tela de progresso, a reconexão ao acordar e a correção do Agent dependem de o Snow **controlar o Battle.net de forma confiável**. Tudo isso só pelos meios que um usuário teria: o executável e os parâmetros dele, ferramentas do próprio Wine, processos e logs. **Nunca** pelos protocolos da Blizzard (seção 2) e nunca olhando a janela (o Wayland não deixa; seção 5).

| Capacidade | Como | Estado |
|---|---|---|
| Abrir | `umu-run Battle.net.exe` no prefixo, com o ambiente da casinha | **Validado** (E6, O6) |
| Saber que está rodando | Processos do prefixo por `WINEPREFIX` e linha de comando | **Validado** (O1) |
| Saber em que etapa está (iniciando, aguardando login, pronto, travado) | Marcas nos logs do cliente: `Logging started` → `AgentManager is now initialized` → `Login triggered` → `State changed to: Ready` / `Logged into Battle.net successfully`. "Aguardando login" = não chegou ao `Ready` em N s | **Validado** (E10, O8) |
| Trazer a janela de volta (inclusive da bandeja) | Rodar o `Battle.net.exe` de novo com ele já aberto: a instância existente se mostra, sem abrir outra | **Validado** (E9, O8) |
| Abrir um jogo direto | `Battle.net.exe --exec="launch <código do produto>"`. O código é o do produto (`WoWF`), **não** o *uid* do Agent (`wowf`, que é ignorado em silêncio); o log liga os dois (`productCode=WoWF_wowf`) | **Validado** (E5, O8) |
| Fechar de forma limpa | `taskkill /im Battle.net.exe` sem `/f`: fecha de verdade em ~1 s (diferente do "X", que só esconde na bandeja), e o Agent sai sozinho em ~10 s. `wineserver -k` e sinais ficam só como plano B (seção 9.2) | **Validado** (E10, O8); em uso no `snowctl house quit` e na bandeja, recusado com jogo aberto (O13). Uma sessão travada ignora o pedido; `wineserver -k` a encerrou (O15) |
| Nunca fechar um jogo aberto | Checar os jogos do prefixo antes de qualquer encerramento (já existe no `snowd`) | **Base pronta** |

Cada capacidade vira uma operação do `snowd` exposta no D-Bus (hoje a interface só tem propriedades de leitura), com `--dry-run` no `snowctl`, e só entra em uso automático depois de validada.

### Identidade visual e paleta

**A metáfora central é a casinha de cachorro.** O Snow Launcher cuida da "casinha" onde o Battle.net mora (seção 5), e a identidade visual mostra isso literalmente: o Snow e a casinha dele. A casinha fica nas ilustrações, **não no formato da janela**: a janela principal continua retangular e nativa do libadwaita, para se comportar bem no GNOME (maximizar, encaixar, sombra, acessibilidade).

**Ícone do app:** a **cabeça do Snow saindo pela porta da casinha**, com chapéu de mago azul e lua dourada, a mancha caramelo no olho esquerdo e a piscadinha no olho direito. Telhado azul-noite com neve, madeira em tons de caramelo. É uma silhueta simples, legível em 32 px ou menos, o que **resolve a pendência** do ícone de corpo inteiro perder detalhe em tamanhos pequenos. Arquivo: `data/icons/io.github.ninjairocode.SnowLauncher.svg`.

**Snow de corpo inteiro (o primeiro ícone, agora ilustração em `data/illustrations/snow-wizard.svg`):** boxer branco sentado num montinho de neve, mancha caramelo no olho esquerdo, piscadinha no olho direito, sorriso torto com dentinho, chapéu de mago azul com lua dourada e cajado com um floco de neve azul na ponta. Traço suave, cores chapadas, sem fundo. Usado nas telas grandes, abaixo.

**Onde a casinha aparece:**

| Lugar | Ilustração |
|---|---|
| Tela de progresso (inicialização, seção 5) | O Snow espiando pela porta e **três luzinhas no contorno do telhado que acendem uma por etapa**: "Preparando o ambiente…", "O Battle.net está iniciando…", "Pronto" (`progress-1.svg` a `progress-3.svg`). Casinha de cachorro não tem janela, por isso luzinhas |
| Tela vazia (nenhum jogo) | A casinha com a porta aberta, ninguém dentro e a tigelinha do lado: "Nenhum jogo ainda" (`empty.svg`) |
| Tela de erro | A casinha apagada e o Snow do lado de fora com um "?", com o diagnóstico e a correção sugerida (`error.svg`) |
| "Sobre" | O Snow inteiro, de mago, ao lado da casinha, com a dedicatória (`about.svg`) |
| README e site | Ícone no topo; o Snow e a casinha como ilustração principal |

**Depois (ideia, não compromisso):** uma janelinha de abertura recortada em formato de casinha enquanto o Battle.net inicia. Só quando houver GUI para testar as limitações do Wayland com janelas especiais.

**Arquivos:** ícone em `data/icons/`; ilustrações em `data/illustrations/`, geradas por `data/illustrations/generate.py` a partir das peças compartilhadas (a casinha e o Snow espiando, tirados do ícone, e o Snow de corpo inteiro). Ao mudar uma peça, rodar o gerador para manter tudo coerente.

**Pendências:** a versão simbólica monocromática do ícone, para a barra do GNOME e a bandeja (v0.3). Em 16 px o ícone colorido fica só com a forma da casinha, o que é esperado nesse tamanho.

**Paleta da marca (tirada do ícone):**

| Nome | Hex | Uso |
|---|---|---|
| Azul mago | `#3C84E6` | Cor principal da marca |
| Azul noite | `#2A63BF` | Variação escura |
| Caramelo Snow | `#C9874F` | Destaque, com moderação (um ponto por tela) |
| Dourado estrela | `#F6D32D` | Detalhes e brilhos |
| Branco neve | `#F7F4F1` | Fundo das ilustrações |
| Rosa bochecha | `#F2C4BF` | Toques suaves nas ilustrações |
| Contorno | `#2B2233` | Traço das ilustrações |

**Regras de uso:**
- **A interface do dia a dia é nativa do libadwaita**: botões, listas e fundos seguem o tema do sistema e a **cor de destaque escolhida pelo usuário**. Não forçar o azul da marca nos controles.
- **Status usam as cores semânticas do libadwaita** (sucesso, aviso, erro), nunca as da marca.
- **A paleta da marca aparece onde está a personalidade do app**: tela de progresso com o Snow animado, telas vazias, "Sobre", ícones próprios dos jogos, README e site.
- **Caramelo com moderação**: um ponto de destaque por tela, por exemplo o selo "Recomendada" na lista de versões do Proton. Nas ilustrações, o caramelo fica na **madeira da casinha** e na mancha do Snow, não espalhado pela interface.
- **No `metainfo.xml`**, declarar as cores de marca para as lojas de apps: `#DCE8FA` (tema claro) e `#203A63` (tema escuro).
- Todas as ilustrações devem funcionar em tema claro e escuro (o contorno escuro garante isso).

## 7 – Correções (quirks)

As correções são **declarativas** (TOML) e ficam num **repositório separado** (`snowlauncher-quirks`), baixado periodicamente. Assim, uma correção nova chega a todos os usuários sem lançar versão nova do app.

### Regras de segurança (obrigatórias)

- **Sem shell arbitrário.** Um quirk só pode usar ações de uma lista fechada, implementada em Rust.
- **Pacote assinado** (minisign). O app embute a chave pública e rejeita pacotes sem assinatura válida.
- **Toda ação é reversível**: backup antes de aplicar, registro no histórico e botão "desfazer".
- **Ações de arquivo ficam presas ao prefixo**: caminhos canonicalizados, sem `..`, sem sair de `drive_c`.
- **Idempotentes**: aplicar duas vezes tem o mesmo efeito que aplicar uma.

### Exemplo de quirk

```toml
schema = 1
id = "battlenet-store-drive-letter"
title = "Loja do jogo: usar letra de unidade em vez de link simbólico"
applies_to = ["battlenet"]
conditions = { proton = ">=GE-Proton9-0" }

[[actions]]
type = "map_drive"
letter = "G"
target = "{games_dir}"

[[actions]]
type = "set_battlenet_setting"
key = "Client/HardwareAcceleration"
value = "false"
```

### Ações permitidas (lista inicial)

`set_windows_version`, `set_registry`, `set_env`, `set_dll_override`, `map_drive`, `set_battlenet_setting`, `clear_battlenet_cache` (só pastas da lista permitida), `pin_runtime`.

## 8 – Diagnóstico e erros conhecidos

O `doctor` lê os logs do Battle.net e do Wine, identifica erros conhecidos e propõe ou aplica a correção:

| Erro | Correção |
|---|---|
| `BLZBNTBNA00000005` (Agent went to sleep) | Encerrar Battle.net e Agent, limpar `ProgramData/Battle.net` (com backup), reabrir. **Só com sintoma** (mensagem visível; instalar, atualizar ou jogar falhando): sob o Wine, o código é registrado em toda inicialização sem afetar nada (`docs/experimentos.md`, O4) |
| Não reconecta após suspender | Reinício limpo após o sinal de "acordar" e a volta da rede |
| Loja não abre | Quirks de letra de unidade, versão do Windows e aceleração por hardware |
| Update preso em 0% / "aguardando outra instalação" | Encerrar processos órfãos do prefixo, limpar locks, reabrir |

O catálogo fica em `snowlauncher-core` (`errors.rs`), com uma **gravidade** por erro (inofensivo ou problema), e cresce também pelo repositório de quirks. O `snowctl doctor` já lê os logs do cliente (`battle.net-*.log`) e conta em quantas sessões cada código aparece; códigos fora do catálogo são listados sem explicação inventada.

**Exportar diagnóstico**: gera um `.zip` com logs, versão do runtime, quirks ativos e dados do sistema. **E-mails, nomes de usuário e caminhos do home devem ser removidos** antes de gerar o arquivo.

## 9 – Qualidade e prova de erros

### 9.1 Regras de código

- `#![forbid(unsafe_code)]` em todos os crates (exceções exigem justificativa escrita).
- **Clippy rígido** no workspace: `unwrap_used`, `expect_used`, `panic` e `todo` negados fora de testes. Habilitar `clippy::pedantic` aos poucos.
- `rustfmt` obrigatório. **MSRV** (versão mínima do Rust) definida e testada no CI.
- Erros tipados com `thiserror` nas bibliotecas. `anyhow` só nos binários, sempre com contexto (`.context("...")`).
- **Nada de `Command` com string de shell.** Sempre argumentos separados.

### 9.2 Segurança ao mexer no sistema do usuário

- **Só tocar processos do prefixo do Snow Launcher.** Identificar processos lendo `/proc/<pid>/environ` (`WINEPREFIX`). Nunca encerrar por nome, para não derrubar um Battle.net do Lutris ou da Steam.
- **Encerramento gradual**: pedido normal → espera com timeout → `SIGTERM` → `SIGKILL`. Usar `wineserver -k` com o mesmo runtime e prefixo.
- **Escrita atômica** de configs e históricos: escrever em arquivo temporário, `fsync`, depois `rename`.
- **Instância única** do daemon (flock em `$XDG_RUNTIME_DIR/snow-launcher.lock`).
- **Mover jogos com segurança**: `rename` no mesmo disco; entre discos, copiar, verificar e só então oferecer apagar o original. Checar espaço livre antes e garantir que nenhum processo do launcher de origem está aberto.
- **Configuração versionada** (`schema = N`) com migrações testadas. Nunca falhar por campo desconhecido: avisar e seguir.
- **Downloads verificados**: GE-Proton conferido por checksum publicado. Quirks conferidos por assinatura. Download parcial nunca substitui a versão em uso.
- **Proteção contra loops**: limite de reinícios automáticos (ex.: 3 em 10 minutos). Depois disso, para e avisa o usuário.
- **Modo `--dry-run`** em todo comando que altera algo: mostra o que faria sem fazer.

### 9.3 Testes

- **Abstrações com traits** para tudo que toca o mundo externo (`ProcessTable`, `SleepMonitor`, `Clock`, `FileSystem`, `Downloader`), com implementações falsas nos testes.
- **Testes unitários** do supervisor com cenários simulados: suspender com jogo aberto, Agent em loop, rede demorando a voltar etc.
- **Testes de snapshot** (`insta`) para parsing de quirks, logs e relatórios do doctor.
- **Testes de propriedade** (`proptest`) para validação de caminhos (nunca sair do prefixo).
- **Testes de integração** em diretórios temporários (prefixo falso).
- **Roteiro de testes manuais** em `docs/testes-manuais.md`:
  - suspender com Battle.net ocioso
  - suspender com WoW aberto
  - hibernar
  - Battle.net se atualizando durante o uso
  - queda de internet
  - dois monitores com DPI diferente
  - primeira instalação do zero
  - voltar runtime para a versão anterior

### 9.4 CI (GitHub Actions)

Feito (`.github/workflows/`):

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo nextest run` e doctests, tudo com `--locked`
- Checagem na MSRV (1.85) além da versão estável
- `cargo deny` (`deny.toml`): vulnerabilidades da base RustSec, licenças compatíveis com GPL-3.0, versões `*` proibidas, só crates.io. Roda em todo push/PR e **toda semana** só para vulnerabilidades, porque elas aparecem sem o código mudar. O `cargo audit` não foi adicionado: usa a mesma base RustSec
- Testes que precisam da sessão de desktop (portal, notificações, NetworkManager, D-Bus) são `#[ignore]` e rodam à mão com `cargo test -- --ignored`

Pendente:

- Build do AppImage a cada tag, com checksum `SHA256SUMS` publicado

### 9.5 Observabilidade

- `tracing` com logs estruturados, enviados ao **journald** e a arquivo rotativo em `~/.local/state/snow-launcher/`.
- **Sem telemetria.** Nada sai da máquina do usuário sem ação explícita dele.
- *Panic hook* que grava o erro no log e avisa por notificação.

### 9.6 Documentação e comunidade

- `README.md`: o que é, o que não é, aviso de não afiliação à Blizzard, selo "Projeto Assistido por IA".
- `CONTRIBUTING.md`: padrão de commits, regra do `Assisted-by:`, como criar um quirk.
- `SECURITY.md`: como reportar vulnerabilidades (relevante, porque o app baixa e aplica correções).
- `CODE_OF_CONDUCT.md`.
- `CHANGELOG.md` no formato *Keep a Changelog*, versionamento SemVer.
- `docs/adr/`: registros curtos de decisões de arquitetura (por que Rust, por que umu, por que não cliente alternativo...).

## 10 – Distribuição

### Fase inicial: AppImage

- Publicado no **GitHub Releases** com `SHA256SUMS`.
- Usar o **runtime estático novo do AppImage** (type2-runtime), que **não depende de libfuse2** no sistema. Isso evita o problema clássico no Fedora.
- **GTK4/libadwaita do sistema (validado no E7, O10)**: um AppImage de 1,2 MB, **sem embutir o GTK**, rodou no Fedora 45, no Ubuntu 24.04 e no Arch. O binário usa só a API base do GTK e exige glibc 2.34. Embutir o GTK (`linuxdeploy` + plugin GTK) fica como plano B para distros sem libadwaita.
- **Montagem sem o `appimagetool`** (que não publica hash nem assinatura): o runtime `type2-runtime` numa **tag fixa** (hoje `20251108`), com a **assinatura GPG conferida** contra a impressão digital `570C 77AC EA40 C0F1 B758 902C BF96 CCA5 6490 F695` gravada no repositório, seguido do squashfs gerado pelo `mksquashfs` da distro. O único binário de fora que roda é o runtime verificado.
- **Lições do E7 para a GUI (corrigidas no O11):**
  - sem o DPI de texto do GTK definido (`gtk-xft-dpi = -1`, quando não há serviço de configurações: gerenciadores de janela mínimos, contêineres), o `adw::Clamp` em "sp" (também usado dentro do `adw::StatusPage`) **colapsa a coluna** para a largura de uma palavra. A GUI usa pixels nesse caso. Não é defeito da libadwaita 1.5;
  - testar toda tela na base mínima (Ubuntu 24.04), não só no Fedora;
  - funcionar também com desenho por software (sem GPU);
  - uma ilustração SVG nunca pode derrubar o app se faltar o `librsvg`;
  - **compilar o AppImage oficial no Ubuntu 24.04** (o CI usa `ubuntu-24.04`): um binário da GUI compilado no Fedora já exige glibc 2.39, sem nenhuma margem.
- **Atualização**: publicar `.zsync` e informações de update embutidas (compatível com AppImageUpdate/Gear Lever).
- **Integração com systemd + AppImage**: a unidade do serviço precisa apontar para o caminho do AppImage, que pode mudar. A cada execução, o app confere `$APPIMAGE` e **atualiza a unidade se o caminho mudou**.
- Arquivos `.desktop` e `metainfo.xml` com o ícone `io.github.ninjairocode.SnowLauncher.svg`.

### Depois

1. **Fedora COPR** (RPM). A política do Fedora permite contribuições assistidas por IA, com transparência. Conferir as regras atuais do COPR antes.
2. **AUR**.
3. **Repositório Flatpak próprio** (`.flatpakref`). **Não usar o Flathub**: a política de maio de 2026 proíbe apps com código assistido por IA, com exceção só para projetos maduros, e mesmo assim sem garantia. Também é proibido usar IA no próprio envio. Nunca esconder o uso de IA para tentar entrar.
   - Testar cedo as permissões de sandbox: barramento do sistema (logind), pastas de jogos, controle do Wine.

## 11 – Git, commits e transparência sobre IA

- **Os commits são feitos pelo mantenedor.** O Claude sugere a mensagem e o mantenedor revisa e commita.
- Padrão **Conventional Commits**, com mensagens em inglês (alcance internacional):

```
feat(daemon): restart Battle.net cleanly after resume

Listen to logind PrepareForSleep and restart Battle.net and Agent
once the network is back. Never kills a running game.

Assisted-by: Claude (Anthropic)
```

- **Trailer `Assisted-by:`** em todo commit com ajuda significativa de IA, seguindo a recomendação do Fedora.
- **Não usar** `Co-Authored-By` do Claude. O padrão do projeto é `Assisted-by:`.
- Branch principal protegida. Mudanças entram por PR, com o CI passando.
- Tags `vX.Y.Z` disparam o build de release.

## 12 – Roadmap

| Versão | Entrega | Estado |
|---|---|---|
| **v0.0** | Experimentos (seção 13) na máquina do mantenedor. Sem app ainda | Parcial: E4 em parte, mais as observações O1–O4 |
| **v0.1** | `snowd` + `snowctl` (CLI): detectar um prefixo existente (ex.: do Lutris), vigiar processos, sleep-hook, sem suspensão automática com jogo aberto + aviso de conexão perdida, `config.toml` básico, correção do erro do Agent, `snowctl status`, `snowctl doctor` | **Quase pronta** (ver 12.1) |
| **v0.2** | Casinha própria: criar prefixo via umu, Proton escolhido, fixado e com volta segura, instalar Battle.net (instalador verificado, arquivo local ou importação de jogos, ver seção 5), aplicar ajustes padrão (Windows 11, **escala automática**), opções globais de **MangoHud e GameMode** | **Pronta** (ver 12.2) |
| **v0.3** | GUI (Relm4/libadwaita): barra de status, tela de jogos instalados com "Jogar" e os **ícones dos jogos tirados dos executáveis**, tela de progresso de inicialização com o Snow e as luzinhas, configurações, notificações, **bandeja só com o Snow**, controle do Battle.net validado (E5, E9, E10) | **Pronta** (ver 12.3) |
| **v0.4** | Repositório de quirks remoto e assinado, lista de jogos com status de compatibilidade, desfazer correções | Não começou |
| **v0.5** | Gamescope por jogo, perfis isolados por jogo (MangoHud por jogo não dá: o jogo herda o ambiente do Battle.net, O16) | Não começou. A escolha do Proton com backup do prefixo veio antes, na v0.2 |
| **v0.6** | Exportar diagnóstico, internacionalização (pt-BR e en), polimento | Não começou. O ícone simbólico saiu do plano: o mantenedor prefere o colorido, e ele ficou nítido na bandeja (E12) |
| **v1.0** | Estável. Avaliar COPR, AUR e repositório Flatpak próprio | Não começou |

### 12.1 Estado da v0.1 (2026-11-27)

**Pronto:**

- Identificação dos processos do prefixo por `WINEPREFIX` (canonicalizado, por causa do link `pfx` do umu/Proton) e pela linha de comando, não pelo `comm`, que o Chromium do Battle.net renomeia
- Sleep-hook (logind) e espera pela rede (NetworkManager)
- Sem suspensão automática com jogo aberto, liberável no `config.toml`
- **Vigilância das conexões do jogo durante toda a sessão:** depois de uma queda longa (suspensão, cabo, roteador), o servidor encerra a sessão e o jogo fica com uma conexão "meio aberta" por ~15 minutos. O `snowd` detecta isso em `/proc/<pid>/net/tcp`, sem root, e avisa uma vez. Retransmissões com a rede local fora não contam. Ficou maior do que o planejado, e veio dos testes reais (`docs/experimentos.md`, O3)
- `config.toml` com `prefix`, compartilhado pelo `snowd` e pelo `snowctl`
- `snowctl status`, `snowctl doctor` (logs do Battle.net e catálogo de erros) e `snowctl service install|uninstall`
- Instância única do `snowd`, serviço `systemd --user`, interface D-Bus de leitura
- CI completo (seção 9.4)

**Pendente, e por quê:**

- **Reiniciar Battle.net e Agent ao acordar:** nos testes, o Battle.net se reconectou sozinho depois de suspensões curtas (O2). Implementar só quando o uso real mostrar o problema; os logs do serviço ficam no journal para isso
- **Correção automática do `BLZBNTBNA00000005`:** o código aparece em toda inicialização sem afetar nada (O4). A correção só deve agir com sintoma visível (seção 8)
- ~~Encerramento gradual de processos~~: **feito** no `snowctl house quit --force` (pedido normal, SIGTERM e SIGKILL pelo `wineserver` da casinha, nunca com jogo aberto; O15). Era pré-requisito das duas acima, que seguem esperando o uso real

### 12.2 Estado da v0.2 (2026-11-28)

**Pronto:** `house create` (umu, Windows 11, DPI do desktop, ícone do Battle.net escondido, `G:`) e `house remove`; instalação do Battle.net com o instalador verificado pela assinatura da Blizzard, a cópia guardada e a vigia; `house open`, `play` (também com o Battle.net fechado), `quit` e `quit --force`; escolha do Proton (`runtime list|set`) com backup do prefixo por reflink e `restore-prefix`; importação de jogos de outros launchers (`snowctl import`); MangoHud e GameMode (`[performance]`).

**Decisões sobre o que estava pendente** (tomadas pelo Claude com a autorização do mantenedor, 2026-11-28):

- **GE-Proton baixado pelo próprio Snow: substituído.** O ProtonPlus e o ProtonUp-Qt já baixam e atualizam os Protons nas pastas da Steam, e o Snow já lista todos, escolhe, **fixa** (o Proton gravado no registro da casinha não muda sozinho) e **volta atrás com segurança** (backup do prefixo por reflink e `restore-prefix`). Baixar pela rede dentro do Snow traria código de rede, verificação de releases e manutenção, sem ganho real. Fica como ideia, se usuários sem ProtonPlus pedirem. A pasta `~/.local/share/snow-launcher/runtimes/` continua sendo procurada, para quem colocar um Proton ali.
- **`snowd` vigiando o prefixo do Lutris: cancelado.** Fazia sentido antes da casinha e da importação; agora os jogos vêm para a casinha, e é ela que o `snowd` vigia. O `--prefix` do `snowd` continua existindo para quem quiser apontar para outro prefixo.

### 12.3 Estado da v0.3 (2026-11-28)

**Pronto:** janela com o estado do `snowd` e da casinha, as luzinhas da inicialização (lidas do log do Battle.net), os jogos com o ícone tirado do próprio executável e "Play", "Open Battle.net", instalar o Battle.net e importar jogos pela janela, Preferências (MangoHud, GameMode, Proton, ícone do Battle.net, iniciar com a sessão, abrir o Battle.net quando o Snow inicia) e "Quit Snow Launcher"; bandeja só com o Snow (E12, E14), com "Open Snow Launcher", "Open Battle.net", "Play", "Quit Battle.net" (só com ele aberto) e "Quit Snow Launcher"; ilustrações nítidas em qualquer escala; base mínima GTK 4.14 e libadwaita 1.5 (Ubuntu 24.04, recurso `gnome_46` do Relm4).

**Polimento visual (feito):** o topo mostra a casinha, o estado do Battle.net e **uma** ação principal em destaque ("Open Battle.net", ou "Show Battle.net" com ele aberto); os jogos vêm logo abaixo, num grupo com "Import" no cabeçalho; o estado fica no fim, em linguagem de quem joga e com ícones; cabeçalho só com o título e o menu (Preferences, About, Quit Snow Launcher); diálogo "About" com o aviso de não afiliação à Blizzard e o crédito à IA. As ilustrações ficam no tamanho lógico em qualquer escala (a textura tem o dobro de pixels com escala 2, e o `GtkPicture` usaria esses pixels como tamanho). Conferido no tema escuro e claro, e no Ubuntu 24.04.

O portal Background deixou de ser necessário: o `snowd` roda como serviço e a bandeja é dele.

## 13 – Experimentos a validar antes de codar

Os experimentos (E) são testes manuais que confirmam se as premissas valem. Registrar os resultados em `docs/experimentos.md`.

Além dos experimentos planejados abaixo, o `docs/experimentos.md` registra observações que surgiram no caminho (O1–O4): processos do Battle.net sob o Proton, sinais de suspensão, conexões meio abertas do WoW e o erro do Agent.

| # | Experimento | Pergunta | Estado |
|---|---|---|---|
| E1 | Desligar a aceleração por hardware do Battle.net | A interface melhora de forma perceptível? | Pendente |
| E2 | GE-Proton recente com NTSYNC ativo | Melhora a resposta geral? `/dev/ntsync` está acessível no Fedora 45? | **Acessível e já em uso** pelo GE-Proton11-14, sem configuração (O5). Falta medir o ganho |
| E3 | Letra de unidade em vez de link simbólico + versão do Windows | A loja do WoW volta a funcionar? Qual combinação? | **Forte indício: Windows 11** faz a loja abrir; a letra de unidade não foi o fator (O6). Falta o teste inverso |
| E4 | Script que escuta `PrepareForSleep` e reinicia o Battle.net | Resolve a reconexão? Reiniciar o Battle.net com o WoW aberto causa algum problema? | Em parte: o sinal chega e o Battle.net reconecta sozinho depois de suspensões curtas (O2). Falta suspensão longa e reiniciar com o WoW aberto |
| E5 | `--exec="launch WoW"` com o Battle.net já aberto, e com gamescope | O jogo herda o ambiente de quem? Dá para aplicar gamescope por jogo sem abrir o Battle.net dentro do gamescope? | **Quase todo validado** (O8, O16): `--exec="launch WoWF"` abre o jogo (código do produto, não o *uid*), e o jogo herda o ambiente do Battle.net. Falta: gamescope por jogo |
| E6 | Instalar o Battle.net via `umu-run` do zero, mover a pasta do WoW do Lutris para uma biblioteca mapeada como `G:` | O instalador trava? Em que ponto? O "Localizar jogo" em `G:\World of Warcraft` reaproveita os arquivos sem baixar tudo de novo? A loja do jogo funciona com o jogo em `G:`? | **Respondido** (O6): o instalador não trava (~78 s); o "Localizar jogo" reaproveitou os 49 GB; o jogo roda em `G:`; a loja abre com Windows 11. Testado com `G:` apontando para a pasta existente, sem mover |
| E7 | AppImage mínimo com GTK4/libadwaita | Roda em Fedora, Ubuntu LTS e Arch? | **Validado** (O10, O11): com o GTK do sistema, nas três. A quebra de layout vista no Ubuntu era o contêiner sem `gtk-xft-dpi` (O11), não a libadwaita 1.5. Não testados: Ubuntu de verdade (com GPU e FUSE), Ubuntu 22.04, sessão X11 |
| E8 | DPI do Wine igual ao `Xft.dpi` do desktop (192 com GNOME a 125%) contra 144 e 96 | O Battle.net e o instalador ficam do mesmo tamanho que os outros aplicativos? Qual é o melhor padrão automático? | **Validado no GNOME 125%** (O7): `Xft.dpi` (192) ficou ideal. Faltam outras escalas, KDE, X11 e vários monitores |
| E9 | Esconder o ícone do Battle.net na bandeja (Wine/Battle.net) e reabrir a janela por fora (rodar o `Battle.net.exe` de novo com ele já aberto) | Dá para esconder? A janela volta quando o Battle.net está só na bandeja? | **Validado** (O8, O14): a janela volta, e o ícone some com a política `NoTrayItemsDisplay` (E14) |
| E11 | Instalador e login a 192 DPI com `QT_ENABLE_HIGHDPI_SCALING=0` | Por que o instalador trava e o login quebra a 192? Dá para manter 192? | **Resolvido** (O9): o modo de alta densidade do Qt 5.15 sob o Wine; desligado, tudo funciona e fica legível a 192 |
| E12 | Item próprio na bandeja (StatusNotifierItem + dbusmenu via `zbus`) no GNOME com AppIndicator | O ícone aparece nítido a 192 DPI? O menu e os cliques funcionam? | **Validado** (O12): ícone nítido por nome com SVG solto no `IconThemePath`; menu e cliques ok; clique esquerdo abre o menu |
| E15 | MangoHud e GameMode do sistema com o umu | O jogo enxerga o MangoHud instalado? Dá para ligar o GameMode só no jogo? | **Validado** (O16): o jogo herda `MANGOHUD=1` do Battle.net e carrega o MangoHud do sistema; o GameMode aceita registrar o processo do jogo pelo D-Bus |
| E10 | Fechar o Battle.net por fora (`taskkill` sem `/f`, depois `wineserver -k`) e mapear nos logs as etapas da inicialização | Fecha de verdade ou só vai para a bandeja? O Agent sai junto? Quais linhas de log marcam "aguardando login" e "pronto"? | **Validado** (O8): fecha de verdade, o Agent sai em ~10 s, marcas mapeadas |

## 14 – Riscos

| Risco | Mitigação |
|---|---|
| Atualização da Blizzard quebra o Battle.net no Wine | Runtime fixado + voltar versão + quirks remotos |
| Blizzard ver o app como violação dos Termos de Uso | Não tocar protocolos, credenciais nem jogos. Só configura o ambiente, como Lutris e Faugus |
| Correção remota maliciosa (cadeia de suprimentos) | Quirks declarativos, assinados e sem shell |
| Encerrar processo errado ou perder progresso de jogo | Identificação por `WINEPREFIX`, nunca encerrar jogo aberto, encerramento gradual |
| Empacotar GTK4 em AppImage | Experimento E7 cedo, matriz de distros no CI |
| Conflito de nome "Snow Launcher" | Pesquisar antes do primeiro release |
| Blizzard muda a página do instalador | O usuário baixa pelo navegador (a página é só um atalho); arquivo local e a cópia verificada guardada continuam funcionando |
| Mantenedor único | Documentação, ADRs, quirks fáceis de contribuir |

## 15 – Instruções para o Claude Code

*(Esta seção pode ser copiada para um `CLAUDE.md` na raiz do repositório.)*

- Leia este `project.md` antes de qualquer tarefa.
- **Nunca** implemente nada que converse com servidores da Blizzard, manipule credenciais, injete código em jogos ou contorne anti-cheat.
- **Não faça commits nem push.** Ao terminar uma tarefa, **sugira** a mensagem de commit no padrão da seção 11, com `Assisted-by: Claude (Anthropic)`, e espere o mantenedor commitar.
- Explique cada mudança antes de aplicar: o mantenedor quer entender e aprender Rust no processo.
- Siga as regras da seção 9 (sem `unwrap`, erros tipados, traits para IO, testes junto com o código).
- Toda ação que altera o prefixo ou o sistema precisa de: dry-run, backup, reversão e teste.
- Prefira mudanças pequenas e incrementais, uma por PR.
- Código, identificadores e mensagens de commit em inglês. Documentação de usuário em pt-BR e en.

## 16 – Referências

- ArcaneLauncher (cliente alternativo, abordagem que o Snow Launcher evita): https://codeberg.org/symb0l/arcane-launcher
- Faugus Launcher: https://github.com/Faugus/faugus-launcher
- ProtonPlus: https://github.com/Vysp3r/ProtonPlus
- Bug do Wine: loja do WoW trava (correção com letra de unidade): https://bugs.winehq.org/show_bug.cgi?id=47778
- Bug do Wine: loja do Battle.net e versão do Windows: https://bugs.winehq.org/show_bug.cgi?id=40931
- Bug do Wine: "Blizzard agent went to sleep": https://bugs.winehq.org/show_bug.cgi?id=45814
- Política de IA do Flathub (maio de 2026): https://www.gamingonlinux.com/2026/05/flathub-moves-to-ban-nearly-all-apps-and-submissions-made-with-generative-ai/
- Política de contribuições assistidas por IA do Fedora: https://docs.fedoraproject.org/en-US/council/policy/ai-contribution-policy/
