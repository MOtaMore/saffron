# Skins del jugador

Texturas UV para el modelo `models/player/Player.glb`. Formato PNG 32×32 con
transparencia (alpha MASK), sin mipmaps; el layout de UVs lo define el propio
GLB (exportado con Blockbench).

**En uso.** `src/player.rs` → `apply_player_skin` pinta las mallas del modelo con
la skin indicada por la constante `PLAYER_SKIN`.

| Archivo | Nota |
|---|---|
| `motamore_skin.png` | skin por defecto |

Para añadir otra skin: mete el PNG aquí y apunta `PLAYER_SKIN` a ella (o, más
adelante, un selector en el menú).
