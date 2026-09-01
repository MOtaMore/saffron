# Texturas de bloques

Aquí van las texturas de las caras de los bloques (grava, piedra, madera, hojas,
cofre, horno, banco de trabajo, cristal, etc.).

Convención sugerida: `<nombre_bloque>.png` para una textura única por bloque, o
`<nombre_bloque>_<cara>.png` (`top`, `bottom`, `side`, `front`) cuando haga falta.

Formato: PNG, potencia de 2 (16×16 o 32×32), sin mipmaps pregenerados.
El motor ya carga `ImagePlugin::default_nearest()` (filtrado nearest, sin desenfoque).

**En uso.** `src/block_atlas.rs` monta en runtime un atlas de 15 columnas de
16×16 con estos archivos:

| Columna | Origen |
|---|---|
| 0,1,2 | `grass-spritesheet.png` (lado, arriba, abajo) |
| 3,4 | `wood-spritesheet.png` (lado, tapa) |
| 5 | `leaves.png` |
| 6 | `sand.png` |
| 7 | `stone.png` |
| 8 | `gravel.png` |
| 9 | `wood_planks.png` (bloque **Madera**, `Block::WoodPlanks`) |
| 10 | `plowed_land.png` (bloque **Tierra arada**, `Block::Farmland`) |
| 11 | `snow.png` (bloque **Nieve**, `Block::Snow`) |
| 12 | `glass.png` (bloque **Cristal**, `Block::Glass` — translúcido) |
| 13 | `mother_rock.png` (bloque **Roca madre**, `Block::Bedrock`) |
| 14 | blanco (bloques sin textura) |

Estos PNG también sirven de icono de inventario del bloque como objeto (su **cara
lateral**), vía `block_side_sprite` en `src/item.rs`. Para césped/tronco/tierra
se recorta el tile lateral del spritesheet con `ImageNode.rect`.

**Fuera del atlas** — texturas para plantas/cultivos en *planos en cruz*
(materiales `StandardMaterial` con `base_color_texture`, no van al atlas de
voxels): `tall_grass.png` (pasto silvestre), `potatoes.png` (papa silvestre),
`wheat.png` (cultivo de trigo, en `props.rs`).

Para añadir un bloque con textura: mete el PNG aquí, añade una entrada en
`block_atlas::load_sources`, sube `block::ATLAS_COLS`, y mapea sus caras en
`block::face_tile` + `block::is_textured`.

