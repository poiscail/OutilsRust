"""Dessine l'icône de l'appli (clé à molette blanche sur dégradé rose → violet) aux tailles d'Android."""
import sys
from PIL import Image, ImageDraw

T = 1024
fond = Image.new("RGBA", (T, T))
d = ImageDraw.Draw(fond)
for y in range(T):
    t = y / (T - 1)
    c = tuple(int(a + (b - a) * t) for a, b in zip((255, 95, 162), (123, 77, 255)))
    d.line([(0, y), (T, y)], fill=c + (255,))
masque = Image.new("L", (T, T), 0)
ImageDraw.Draw(masque).rounded_rectangle([0, 0, T - 1, T - 1], radius=220, fill=255)
fond.putalpha(masque)

# clé à molette dessinée verticale, puis tournée de 45°
cle = Image.new("RGBA", (T, T), (0, 0, 0, 0))
k = ImageDraw.Draw(cle)
k.rounded_rectangle([452, 420, 572, 900], radius=60, fill="white")  # manche
k.ellipse([352, 150, 672, 470], fill="white")                        # tête
k.rectangle([472, 120, 552, 300], fill=(0, 0, 0, 0))                 # mâchoire
k.ellipse([482, 800, 542, 860], fill=(0, 0, 0, 0))                   # trou du manche
cle = cle.rotate(-45, resample=Image.BICUBIC, center=(T / 2, T / 2))
fond.alpha_composite(cle)

dossier = sys.argv[1]
for nom, taille in [("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)]:
    fond.resize((taille, taille), Image.LANCZOS).save(f"{dossier}/mipmap-{nom}/ic_launcher.png")
fond.resize((512, 512), Image.LANCZOS).save(f"{dossier}/../icone-512.png")
