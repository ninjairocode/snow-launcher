<p align="center">
  <img src="data/icons/io.github.ninjairocode.SnowLauncher.svg" width="128" height="128" alt="Snow, um boxer branco de chapéu de mago, espiando da casinha">
</p>

<h1 align="center">Snow Launcher</h1>

<p align="center">
  Mantém o Battle.net oficial bem configurado e vigiado no Linux.
</p>

<p align="center">
  <a href="https://github.com/ninjairocode/snow-launcher/actions/workflows/ci.yml"><img src="https://github.com/ninjairocode/snow-launcher/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/licen%C3%A7a-GPL--3.0--or--later-blue" alt="Licença: GPL-3.0-or-later">
  <img src="https://img.shields.io/badge/assistido%20por%20IA-sim-8a2be2" alt="Projeto Assistido por IA">
</p>

<p align="center"><a href="README.md">Read in English</a></p>

---

> **Em desenvolvimento inicial (v0.3).** O Snow Launcher monta e vigia o
> próprio ambiente do Battle.net, com uma janela do GNOME e um ícone na
> bandeja. Funciona na máquina do mantenedor; em outras, espere arestas.

## O que é

Jogar títulos da Blizzard no Linux normalmente significa rodar o Battle.net
no Wine ou no Proton por um launcher genérico. Funciona, mas quebra de jeitos
conhecidos. O Snow Launcher é um companheiro focado nesse cenário:

- **Uma "casinha" própria para o Battle.net.** Um prefixo do Wine feito para
  o Battle.net com o [umu](https://github.com/Open-Wine-Components/umu-launcher)
  e o Proton que você escolher (GE-Proton por padrão, ou qualquer um que o
  ProtonPlus instalou), em Windows 11 e na escala do seu desktop, para o
  Battle.net ficar legível.
- **Instala o Battle.net com segurança.** Nunca embute nem baixa o instalador
  da Blizzard: você baixa no navegador, e o Snow confere a assinatura digital
  da Blizzard antes de rodar.
- **Traz os seus jogos.** Importa um jogo do Lutris ou da Steam para a
  biblioteca dele, numa cópia instantânea no Btrfs/XFS; o original continua
  funcionando.
- **Um clique para jogar.** Uma janela e um ícone na bandeja com os seus
  jogos (os ícones vêm dos próprios arquivos do jogo) e o progresso da
  inicialização do Battle.net.
- **Sem suspensão automática enquanto você joga**, e **um aviso quando a
  conexão do jogo morre** depois de uma queda, em vez de o jogo ficar
  desconectado em silêncio por uns 15 minutos.
- **MangoHud e GameMode opcionais**, usando os que você instalou.
- **Seguro por desenho:** simulação antes (dry-run), backup antes de mudar,
  caminho de volta para tudo, e nunca fecha um jogo aberto.

## O que não é

- **Não é um cliente alternativo do Battle.net.** Nunca conversa com os
  servidores da Blizzard nem mexe nas suas credenciais; o login é sempre pelo
  app oficial.
- **Não é ferramenta de trapaça ou mod.** Não injeta código nos jogos, não lê
  nem altera a memória deles e não contorna anti-cheat.
- Só mexe nos processos do próprio prefixo, identificados pelo `WINEPREFIX`;
  um Battle.net rodando em outro lugar (Lutris, Steam) nunca é tocado.

## Requisitos

- Linux com systemd (logind) e NetworkManager
- GNOME com a extensão AppIndicator para o ícone da bandeja (desenvolvido no
  Fedora; a base mínima é o Ubuntu 24.04: GTK 4.14, libadwaita 1.5)
- [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) (ou o
  Lutris, que já traz o umu) e um GE-Proton (por exemplo, do ProtonPlus)
- [Rust](https://rustup.rs/) 1.85 ou mais novo, e os pacotes de
  desenvolvimento do GTK 4 e da libadwaita, para compilar

## Instalar a partir do código

```sh
git clone https://github.com/ninjairocode/snow-launcher.git
cd snow-launcher
cargo install --locked --path crates/snowlauncher-daemon   # snowd
cargo install --locked --path crates/snowlauncher-cli      # snowctl
cargo install --locked --path crates/snowlauncher-gui      # a janela
snowctl service install   # o snowd como serviço, iniciado com a sessão
```

## Primeiros passos

```sh
snowctl house create --dry-run    # mostra o que faria
snowctl house create              # a casinha: prefixo, Windows 11, escala
```

Depois abra o **Snow Launcher**: baixe o instalador do Battle.net no
navegador ("Open download page"), clique em **Install Battle.net**, faça o
login e importe ou instale os seus jogos. Tudo também existe no `snowctl`
(`snowctl --help`):

```sh
snowctl status            # o que o snowd está vendo
snowctl games             # os jogos da biblioteca
snowctl play wowf         # abrir um jogo pelo Battle.net
snowctl import            # jogos que outros launchers têm
snowctl doctor            # problemas conhecidos nos logs recentes do Battle.net
```

## Como funciona

- O `snowd`, um daemon pequeno, vigia os processos da casinha, escuta a
  suspensão e o retorno (logind) e a rede (NetworkManager), pede ao desktop
  para não suspender enquanto um jogo roda, lê o estado das conexões TCP dos
  jogos em `/proc` (sem root) e mostra o ícone da bandeja.
- O `snowctl` é a ferramenta de linha de comando; o `snow-launcher` é a
  janela do GNOME (Relm4 e libadwaita).
- Eles conversam por D-Bus, no barramento da sessão
  (`io.github.ninjairocode.SnowLauncher1`).

O projeto, as decisões e os motivos estão no [`project.md`](project.md).
Medições do uso real estão em [`docs/experimentos.md`](docs/experimentos.md).

## Projeto Assistido por IA

O Snow Launcher é desenvolvido com a ajuda do Claude (Anthropic). Toda
mudança é revisada, testada e commitada por um mantenedor humano, e os
commits com ajuda significativa de IA levam o trailer `Assisted-by:`.

## Não afiliado à Blizzard

O Snow Launcher é um projeto independente, sem afiliação, endosso ou
patrocínio da Blizzard Entertainment. Battle.net, World of Warcraft e os
outros nomes citados são marcas de seus respectivos donos.

## Licença

[GPL-3.0-or-later](LICENSE).

---

<sub>Para o Snow: boxer branco, mancha caramelo no olho, sorriso torto,
piscadinha de lado. Agora mago oficial do projeto.</sub>
