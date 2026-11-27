"""Builds the Snow Launcher illustrations from the shared pieces: the
doghouse and Snow peeking out of its door (both from the app icon) and
Snow's full-body drawing (snow-wizard.svg).

Run from the repository root after changing any of those pieces:

    python3 data/illustrations/generate.py .
"""
import re, sys
root = sys.argv[1]
icon = open(f"{root}/data/icons/io.github.ninjairocode.SnowLauncher.svg").read()
full = open(f"{root}/data/illustrations/snow-wizard.svg").read()

def between(text, start, end):
    a = text.index(start); b = text.index(end, a)
    return text[a:b]

house = between(icon, "  <!-- montinho de neve -->", "  <!-- Snow espiando")
peek = between(icon, "  <!-- Snow espiando", "  <!-- floquinhos caindo -->")
# Snow de corpo inteiro, sem o montinho e os floquinhos do ícone original.
snow_body = full[full.index("<svg"):]
snow_body = snow_body[snow_body.index(">") + 1: snow_body.rindex("</svg>")]
snow_body = re.sub(r"  <!-- montinho de neve -->.*?(?=  <g stroke)", "", snow_body, flags=re.S)
snow_body = re.sub(r"  <!-- floquinhos caindo -->.*", "", snow_body, flags=re.S)

def nested(content, x, y, size):
    return (f'<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 128 128" '
            f'xmlns:xlink="http://www.w3.org/1999/xlink">\n{content}</svg>\n')

FLAKES = '''<g fill="#9fb6d8">
  <circle cx="22" cy="30" r="2.2"/><circle cx="60" cy="14" r="1.6"/><circle cx="290" cy="26" r="2"/>
  <circle cx="306" cy="92" r="1.6"/><circle cx="14" cy="120" r="1.7"/><circle cx="250" cy="10" r="1.4"/>
</g>
'''
GROUND = '<path d="M 10 200 C 60 182, 260 182, 310 200 C 260 214, 60 214, 10 200 Z" fill="#e3eefc" stroke="#9fb6d8" stroke-width="2" stroke-linejoin="round"/>\n'

def doc(w, h, title, body):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
            f'width="{w}" height="{h}" viewBox="0 0 {w} {h}">\n  <title>{title}</title>\n{body}</svg>\n')

out = {}

# Sobre: o Snow de mago sentado na frente da casinha.
out["about.svg"] = doc(320, 220, "Snow e a casinha",
    GROUND + FLAKES + nested(house, 150, 30, 170) + nested(snow_body, 20, 52, 150))

# Tela vazia: a casinha com a porta aberta e ninguém dentro.
bowl = '''<g stroke="#4a4453" stroke-width="2.2" stroke-linejoin="round">
  <path d="M 196 196 L 200 184 L 232 184 L 236 196 Z" fill="#3c84e6"/>
  <path d="M 203 184 C 206 179, 226 179, 229 184 Z" fill="#c9874f"/>
</g>
'''
out["empty.svg"] = doc(320, 220, "Nenhum jogo ainda",
    GROUND + FLAKES + nested(house, 75, 18, 170) + bowl)

# Erro: a casinha apagada e o Snow do lado de fora, com uma dúvida.
bubble = '''<g transform="translate(116 34)">
  <path d="M 0 22 C 0 8, 10 0, 24 0 C 38 0, 48 8, 48 22 C 48 36, 38 44, 24 44 C 20 44, 16 43, 13 42 L 2 50 L 6 38 C 2 34, 0 28, 0 22 Z" fill="#ffffff" stroke="#4a4453" stroke-width="2.4" stroke-linejoin="round"/>
  <path d="M 17 16 C 17 9, 31 8, 31 16 C 31 22, 24 22, 24 28" fill="none" stroke="#2a63bf" stroke-width="4" stroke-linecap="round"/>
  <circle cx="24" cy="35" r="2.6" fill="#2a63bf"/>
</g>
'''
dark_house = nested(house, 160, 30, 160).replace('fill="#3b2f45"', 'fill="#231c2b"')
out["error.svg"] = doc(320, 220, "Algo deu errado",
    GROUND + FLAKES + '<g opacity="0.72">\n' + dark_house + '</g>\n' + nested(snow_body, 16, 58, 145) + bubble)

# Progresso: o Snow espiando e três luzinhas no beiral, uma por etapa.
def bulbs(lit):
    # Cordão de luzinhas acompanhando o contorno do telhado, por fora,
    # longe do chapéu e da cabeça do Snow.
    wire = '<path d="M 9 61 L 64 13 L 119 61" fill="none" stroke="#4a4453" stroke-width="1.6" stroke-linejoin="round"/>\n'
    # (x, y) do soquete no fio e o ângulo em que a lâmpada pende para fora.
    spots = [(30, 42.7, -41), (64, 13, 0), (98, 42.7, 41)]
    parts = [wire]
    for i, (x, y, angle) in enumerate(spots):
        on = i < lit
        fill = "#f6d32d" if on else "#c9c2bb"
        glow = f'<circle cx="0" cy="-7" r="8" fill="#f6d32d" opacity="0.35"/>' if on else ""
        parts.append(
            f'<g transform="translate({x} {y}) rotate({angle})">{glow}'
            f'<rect x="-2" y="-2.5" width="4" height="3" rx="1" fill="#4a4453"/>'
            f'<ellipse cx="0" cy="-7" rx="3.6" ry="4.6" fill="{fill}" stroke="#4a4453" stroke-width="1.4"/></g>\n')
    return "".join(parts)

# Etapa 0: Battle.net fechado, luzinhas apagadas.
for stage in (0, 1, 2, 3):
    body = house + peek + bulbs(stage)
    title = "Battle.net fechado" if stage == 0 else f"Iniciando, etapa {stage} de 3"
    out[f"progress-{stage}.svg"] = doc(128, 128, title, body)

for name, text in out.items():
    open(f"{root}/data/illustrations/{name}", "w").write(text)
    print("wrote", name)
