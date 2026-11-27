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
| Onde ficam os jogos | Na **biblioteca**, fora do prefixo, mapeada como `G:`. Importar sempre **move** o jogo para a biblioteca |
| Transparência | "Projeto Assistido por IA" no README + trailer `Assisted-by:` |
| Commits | Feitos manualmente pelo mantenedor, com orientação do Claude |
| Licença | **GPL-3.0-or-later**, a mesma família do umu-launcher, comum no ecossistema GNOME (arquivo `LICENSE`) |
| App ID | `io.github.ninjairocode.SnowLauncher` |
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
| `~/.local/share/snow-launcher/runtimes/` | Versões do GE-Proton baixadas |
| `~/.local/share/snow-launcher/quirks/` | Cache do repositório de correções |
| `~/.local/state/snow-launcher/` | Logs, histórico de correções aplicadas, backups |
| `~/.cache/snow-launcher/` | Downloads temporários |
| `~/Games/SnowLauncher/` | **Biblioteca de jogos** (padrão, configurável). Fica fora dos prefixos e aparece como `G:` na casinha |

## 5 – A casinha (ambiente do Battle.net)

A casinha é um **prefixo Wine controlado pelo Snow Launcher**. Ela inclui:

- **Runtime fixado**: uma versão do GE-Proton testada e fixada, com **voltar para a anterior em um clique** se uma atualização quebrar algo. O usuário pode escolher outra versão (ver "Escolha do Proton e integração com o ProtonPlus").
- **Ajustes padrão do prefixo**:
  - Aceleração por hardware do navegador do Battle.net **desligada**.
  - Versão do Windows correta para a loja funcionar (validar no experimento E3).
  - Pasta de jogos acessada por **letra de unidade mapeada**, não por link simbólico (correção conhecida da loja).
  - **NTSYNC** quando `/dev/ntsync` existir e for acessível. Caso contrário, fsync/esync, e a GUI informa qual está ativo.
  - **DPI/escala** ajustados ao monitor principal.
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
- Se a versão escolhida for removida do sistema (por exemplo, apagada no ProtonPlus), o Snow avisa e oferece voltar para a recomendada, em vez de falhar ao abrir.
- A versão recomendada vem do repositório de quirks, para poder mudar sem lançar versão nova do app.

### Instalação do Battle.net na casinha

O Snow Launcher oferece três caminhos. O assistente de primeira execução pergunta qual usar, começando pela importação, se encontrar algo.

**1 – Download automático (padrão)**
- Baixa o `Battle.net-Setup.exe` direto do **servidor oficial da Blizzard**, na máquina do usuário, na hora da instalação.
- **Nunca** embutir o instalador no app nem hospedar cópia (o arquivo é da Blizzard, redistribuir é problema de direitos autorais).
- O endereço de download **não fica fixo no código**: vem do repositório de quirks, para poder ser corrigido sem lançar versão nova se a Blizzard mudar.
- A versão do instalador quase não importa: depois de instalado, o Battle.net se atualiza sozinho.

**2 – Arquivo local**
- Botão "Usar instalador que já baixei". Plano B se o download falhar ou a rede bloquear o endereço.

**3 – Importar instalação existente (o mais importante para quem já joga)**

Importar sempre **move o jogo para a biblioteca do Snow** (ver "Casinha e biblioteca"). Não existe a opção de usar o jogo no lugar, para o jogo nunca depender da pasta de outro launcher.

1. Detectar prefixos do Lutris, da Steam (compatdata) e do Faugus, e as pastas de jogos Blizzard dentro deles (`World of Warcraft`, `Diablo IV` etc.).
2. Mostrar o que foi encontrado, com tamanho, e explicar antes de confirmar: **"Depois de mover, este jogo deixa de funcionar no Lutris."**
3. Conferir antes de mover:
   - o Battle.net e o jogo **não podem estar abertos** no outro launcher (processos com aquele `WINEPREFIX`);
   - se a biblioteca está em **outro disco**, verificar se há espaço livre suficiente.
4. Mover:
   - **Mesmo disco:** `rename` da pasta. É instantâneo e atômico, não copia nada.
   - **Outro disco:** copiar → verificar (quantidade de arquivos e tamanhos) → só então **oferecer** apagar o original. Nunca apagar antes de confirmar a cópia. Se a cópia for interrompida, a pasta parcial é descartada e o original continua intacto.
5. Abrir a casinha e guiar o usuário no **"Localizar jogo"** do Battle.net, apontando para `G:\<nome do jogo>`. O Battle.net verifica os arquivos e baixa só o que faltar.
6. Registrar no histórico de onde o jogo veio, para facilitar o suporte.

O app Battle.net e os dados do outro launcher **não são importados**: a casinha instala o seu próprio Battle.net. Só os jogos mudam de lugar.

**Verificação do instalador (caminhos 1 e 2)**
- A Blizzard não publica checksum, mas o `.exe` tem **assinatura digital Authenticode**. Antes de executar, conferir se a assinatura é válida e emitida para **Blizzard Entertainment**. Se não for, recusar e explicar o motivo.
- Download em arquivo temporário. Só é usado depois de completo e verificado.

**Instalação vigiada**
- O instalador do Battle.net costuma travar ou ficar em 0% no Wine (o script do Lutris, por exemplo, pede para fechar o app na primeira tela de login).
- O Snow Launcher trata a instalação como um processo vigiado: inicia o instalador, detecta quando terminou ou travou (arquivos criados, processo ocioso, timeout), encerra de forma limpa e reabre o Battle.net já instalado.
- Os detalhes dependem do experimento E6.

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
- **Sem artes ou logos oficiais da Blizzard.** Usar um quadrado com a inicial ou ícones próprios no estilo do Snow.
- No libadwaita: `GtkFlowBox` de cards numa página simples. O Bottles é uma boa referência visual.

### Identidade visual e paleta

O ícone é o Snow de corpo inteiro, como mascote (traço suave, cores chapadas, sem fundo): boxer branco sentado num montinho de neve, mancha caramelo no olho esquerdo, piscadinha no olho direito, sorriso torto com dentinho, chapéu de mago azul com lua dourada e um cajado com um floco de neve azul na ponta. Arquivo: `data/icons/io.github.ninjairocode.SnowLauncher.svg`.

**Pendência:** em tamanhos muito pequenos (32 px ou menos) o corpo inteiro perde detalhes. Avaliar uma versão só com a cabeça para esses tamanhos.

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
- **Caramelo com moderação**: um ponto de destaque por tela, por exemplo o selo "Recomendada" na lista de versões do Proton.
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
- **GTK4/libadwaita dentro do AppImage**: empacotar com `linuxdeploy` + plugin GTK. Testar em Fedora, Ubuntu LTS e Arch. Este é o ponto mais delicado do empacotamento.
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
| **v0.2** | Casinha própria: criar prefixo via umu, baixar e verificar GE-Proton, fixar e voltar versão, instalar Battle.net (download oficial, arquivo local ou importação de instalação existente, ver seção 5), aplicar ajustes padrão | Não começou |
| **v0.3** | GUI (Relm4/libadwaita): barra de status, tela de jogos instalados com "Jogar", tela de progresso de inicialização com o Snow animado, configurações, notificações, portal Background | Não começou |
| **v0.4** | Repositório de quirks remoto e assinado, lista de jogos com status de compatibilidade, desfazer correções | Não começou |
| **v0.5** | Gamescope por jogo, DPI/escala, perfis isolados por jogo, escolha do Proton (pastas da Steam/ProtonPlus) com backup do prefixo | Não começou |
| **v0.6** | Exportar diagnóstico, internacionalização (pt-BR e en), ícone simbólico, polimento | Não começou |
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
- Encerramento gradual de processos (`wineserver -k`, SIGTERM, SIGKILL), pré-requisito das duas acima

## 13 – Experimentos a validar antes de codar

Os experimentos (E) são testes manuais que confirmam se as premissas valem. Registrar os resultados em `docs/experimentos.md`.

Além dos experimentos planejados abaixo, o `docs/experimentos.md` registra observações que surgiram no caminho (O1–O4): processos do Battle.net sob o Proton, sinais de suspensão, conexões meio abertas do WoW e o erro do Agent.

| # | Experimento | Pergunta | Estado |
|---|---|---|---|
| E1 | Desligar a aceleração por hardware do Battle.net | A interface melhora de forma perceptível? | Pendente |
| E2 | GE-Proton recente com NTSYNC ativo | Melhora a resposta geral? `/dev/ntsync` está acessível no Fedora 45? | Pendente |
| E3 | Letra de unidade em vez de link simbólico + versão do Windows | A loja do WoW volta a funcionar? Qual combinação? | Pendente |
| E4 | Script que escuta `PrepareForSleep` e reinicia o Battle.net | Resolve a reconexão? Reiniciar o Battle.net com o WoW aberto causa algum problema? | Em parte: o sinal chega e o Battle.net reconecta sozinho depois de suspensões curtas (O2). Falta suspensão longa e reiniciar com o WoW aberto |
| E5 | `--exec="launch WoW"` com o Battle.net já aberto, e com gamescope | O jogo herda o ambiente de quem? Dá para aplicar gamescope por jogo sem abrir o Battle.net dentro do gamescope? | Pendente |
| E6 | Instalar o Battle.net via `umu-run` do zero, mover a pasta do WoW do Lutris para uma biblioteca mapeada como `G:` | O instalador trava? Em que ponto? O "Localizar jogo" em `G:\World of Warcraft` reaproveita os arquivos sem baixar tudo de novo? A loja do jogo funciona com o jogo em `G:`? | Pendente |
| E7 | AppImage mínimo com GTK4/libadwaita | Roda em Fedora, Ubuntu LTS e Arch? | Pendente |

## 14 – Riscos

| Risco | Mitigação |
|---|---|
| Atualização da Blizzard quebra o Battle.net no Wine | Runtime fixado + voltar versão + quirks remotos |
| Blizzard ver o app como violação dos Termos de Uso | Não tocar protocolos, credenciais nem jogos. Só configura o ambiente, como Lutris e Faugus |
| Correção remota maliciosa (cadeia de suprimentos) | Quirks declarativos, assinados e sem shell |
| Encerrar processo errado ou perder progresso de jogo | Identificação por `WINEPREFIX`, nunca encerrar jogo aberto, encerramento gradual |
| Empacotar GTK4 em AppImage | Experimento E7 cedo, matriz de distros no CI |
| Conflito de nome "Snow Launcher" | Pesquisar antes do primeiro release |
| Blizzard muda o endereço do instalador | Endereço vem do repositório de quirks (atualizável) + opção de arquivo local |
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
