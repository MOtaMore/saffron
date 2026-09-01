# Texturas de herramientas

Iconos de las herramientas para la hotbar y el inventario.

**En uso.** Los mapea `Item::texture_path` (`src/item.rs`); la UI los pinta con
`paint_icon` → `ImageNode`.

| Archivo | Herramienta |
|---|---|
| `flint_knife.png` | Cuchillo de pedernal |
| `flint_axe.png` | Hacha de pedernal |
| `flint_pickaxe.png` | Pico de pedernal |
| `flint_shovel.png` | Pala de pedernal |
| `flint_sickle.png` | Hoz de pedernal |
| `rudimentary_fishingrod.png` | Caña de pescar — icono de inventario |
| `rudimentary_fishingrod-casted.png` | Variante "lanzada" — **sin usar por ahora** |

La caña equipada se dibuja con el modelo 3D `models/RudimentaryFishingrod.glb`,
no con estos iconos. `-casted` queda reservada para cuando haya un icono/estado
distinto al lanzar (aún sin enchufar).

Convención: `<nombre>.png`, PNG con transparencia, 16–32 px.
