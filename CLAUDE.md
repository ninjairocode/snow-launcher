# CLAUDE.md

Instruções para o Claude Code neste repositório. A fonte de verdade do projeto é o [`project.md`](project.md): leia-o antes de qualquer tarefa.

## Limites do projeto (inegociáveis)

- **Nunca** implemente nada que converse com servidores da Blizzard, manipule credenciais, injete código em jogos, leia/altere memória de jogos ou contorne anti-cheat.
- O Snow Launcher **envolve** o Battle.net oficial; nunca o substitui nem imita seus protocolos.
- Sem marcas, artes ou logos da Blizzard em código, ícones ou textos.

## Git

- **Não faça commits nem push.** Ao terminar uma tarefa, **sugira** a mensagem de commit e espere o mantenedor commitar.
- Padrão Conventional Commits, mensagem em inglês, com o trailer `Assisted-by: Claude (Anthropic)`.
- **Não use** `Co-Authored-By`. O padrão do projeto é `Assisted-by:`.

```
feat(daemon): restart Battle.net cleanly after resume

Listen to logind PrepareForSleep and restart Battle.net and Agent
once the network is back. Never kills a running game.

Assisted-by: Claude (Anthropic)
```

## Forma de trabalhar

- Explique cada mudança antes de aplicar: o mantenedor quer entender e aprender Rust no processo.
- Mudanças pequenas e incrementais, uma por PR.
- Código, identificadores e mensagens de commit em inglês. Documentação de usuário em pt-BR e en.

## Regras de código (seção 9 do `project.md`)

- `unsafe_code` proibido (lint do workspace). Exceções exigem justificativa escrita.
- Sem `unwrap`, `expect`, `panic!` ou `todo!` fora de testes.
- Erros tipados com `thiserror` nas bibliotecas; `anyhow` só nos binários, sempre com `.context("...")`.
- **Nada de `Command` com string de shell.** Sempre argumentos separados.
- Tudo que toca o mundo externo fica atrás de traits (`ProcessTable`, `SleepMonitor`, `Clock`, `FileSystem`, `Downloader`), com implementações falsas nos testes.
- Testes junto com o código: unitários, snapshot (`insta`), propriedade (`proptest`) para validação de caminhos, integração em diretórios temporários.

## Segurança ao mexer no sistema do usuário

- Toda ação que altera o prefixo ou o sistema precisa de: **dry-run, backup, reversão e teste**.
- Só tocar processos do prefixo do Snow: identificar por `WINEPREFIX` em `/proc/<pid>/environ`, **nunca por nome**.
- **Nunca encerrar um jogo em execução.** Encerramento gradual: pedido normal → timeout → `SIGTERM` → `SIGKILL`.
- Escrita atômica (temporário → `fsync` → `rename`).
- Ações de arquivo de quirks presas ao prefixo: caminhos canonicalizados, sem `..`, sem sair de `drive_c`.
- Limite de reinícios automáticos (ex.: 3 em 10 minutos).

## Estrutura

| Crate | Binário | Papel |
|---|---|---|
| `snowlauncher-core` | — | Domínio: tipos, quirks, catálogo de erros, regras. Sem IO direto |
| `snowlauncher-wine` | — | Prefixo, runtime (umu/GE-Proton), registro, processos Wine |
| `snowlauncher-daemon` | `snowd` | Vigia, sleep-hook, aplicador de correções, D-Bus |
| `snowlauncher-cli` | `snowctl` | Cliente D-Bus: status, doctor, launch, quirks |
| `snowlauncher-gui` | `snow-launcher` | Relm4 + libadwaita (previsto para v0.3, ainda não existe) |

Prefixo `snowlauncher-` nos crates para não confundir com o crate `snow` (criptografia) do crates.io.

## Comandos

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace        # o CI usará cargo nextest run
```
