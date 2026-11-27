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

> **Em desenvolvimento inicial.** Hoje o Snow Launcher trabalha com um
> prefixo do Battle.net que você já tem (por exemplo, do Lutris). Ainda não
> há interface gráfica, e ele não cria nem instala nada por você.

## O que é

Jogar títulos da Blizzard no Linux normalmente significa rodar o Battle.net
no Wine ou no Proton por um launcher genérico. Funciona, mas quebra de jeitos
conhecidos. O Snow Launcher é um companheiro focado nesse cenário:

- **Sem suspensão automática enquanto você joga.** Com um jogo aberto, o
  computador não dorme sozinho (a tela ainda pode apagar). Dá para desligar
  isso.
- **Avisa quando a conexão do jogo morre.** Depois de uma queda de rede longa
  o bastante (suspensão, cabo, roteador), o servidor encerra a sua sessão,
  mas o jogo continua aberto e pode levar uns 15 minutos para perceber: dá
  até para andar com o personagem, mas nada chega ao servidor. O Snow
  Launcher percebe em menos de um minuto e avisa para reiniciar o jogo.
- **Explica erros conhecidos.** O `snowctl doctor` lê os logs do próprio
  Battle.net e explica os códigos de erro que conhece, sem chutar os outros.

O plano é virar um gerenciador completo de um ambiente dedicado ao
Battle.net, com runtime Proton próprio, correções automáticas e interface
GNOME. Veja o [roadmap](project.md#12--roadmap).

## O que não é

- **Não é um cliente alternativo do Battle.net.** Nunca conversa com os
  servidores da Blizzard nem toca nas suas credenciais; o login é sempre na
  janela oficial.
- **Não é ferramenta de trapaça ou mod.** Não injeta código nos jogos, não lê
  nem altera a memória deles e não contorna anti-cheat.
- Só olha os processos do prefixo Wine configurado, então um Battle.net
  rodando em outro lugar (Lutris, Steam) nunca é afetado.

## Requisitos

- Linux com systemd (logind) e NetworkManager
- Desktop com o portal XDG e notificações (desenvolvido no GNOME / Fedora)
- Um prefixo Wine/Proton existente com o Battle.net instalado
- [Rust](https://rustup.rs/) 1.85 ou mais novo, para compilar

## Instalar a partir do código

```sh
git clone https://github.com/ninjairocode/snow-launcher.git
cd snow-launcher
cargo install --locked --path crates/snowlauncher-daemon   # snowd
cargo install --locked --path crates/snowlauncher-cli      # snowctl
```

Diga onde está o seu prefixo do Battle.net, em
`~/.config/snow-launcher/config.toml`:

```toml
schema = 1
prefix = "~/Games/battlenet"

[sleep]
# true para deixar o computador suspender sozinho mesmo durante o jogo.
allow_suspend_while_playing = false
```

Depois, rode o daemon como serviço do usuário, para ele subir com a sessão:

```sh
snowctl service install --dry-run   # mostra o que faria
snowctl service install
```

Para remover: `snowctl service uninstall`.

## Uso

```sh
snowctl status    # processos do Battle.net e dos jogos, e o que o snowd está vendo
snowctl doctor    # problemas conhecidos nos logs recentes do Battle.net
journalctl --user -u snowd -f   # log do daemon
```

## Como funciona

- O `snowd`, um daemon pequeno, vigia os processos do prefixo, escuta a
  suspensão (logind) e a rede (NetworkManager), pede ao desktop para não
  suspender enquanto um jogo roda e lê o estado das conexões TCP do jogo em
  `/proc` (sem root).
- O `snowctl` é a ferramenta de linha de comando.
- Os dois conversam por D-Bus, no barramento de sessão
  (`io.github.ninjairocode.SnowLauncher1`).

O desenho, as decisões e os motivos estão no [`project.md`](project.md). As
medições feitas no uso real estão em
[`docs/experimentos.md`](docs/experimentos.md).

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
