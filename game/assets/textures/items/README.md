# Texturas de items y de bloques como objeto

Iconos para el inventario/hotbar de los items que no son herramientas y de los
bloques cuando se muestran como objeto recogido.

**En uso.** Los cargan `Item::texture_path` (`src/item.rs`) y `paint_icon` vía
`Item::inventory_sprite`, que pone un `ImageNode` en cada casilla; si un item no
tiene PNG se recurre al cuadro de color de `Item::icon_color`.

| Archivo | Item |
|---|---|
| `flint.png` | Pedernal |
| `stick.png` | Palo |
| `plant.png` | Fibra vegetal |
| `rope.png` | Soga de plantas |
| `raw_fish.png` | Pescado |
| `cooked_fish.png` | Pescado cocinado |
| `charcoal.png` | Carbón vegetal |
| `flour.png` | Harina |
| `dough.png` | Masa |
| `bread.png` | Pan |
| `meat.png` | Carne (cerdo) |
| `fat.png` | Grasa |
| `read_meat.png` | Carne roja (vaca) |
| `leather.png` | Cuero |
| `lamb_meat.png` | Cordero (oveja) |
| `wool.png` | Lana |
| `white_meat.png` | Carne blanca (pollo) |
| `feather.png` | Pluma |
| `meat-cooked.png` | Carne asada (cerdo, `Item::CookedMeat`) |
| `read_meat-cooked.png` | Carne roja asada (`Item::CookedRedMeat`) |
| `lamb_meat-cooked.png` | Cordero asado (`Item::CookedMutton`) |
| `white_meat-cooked.png` | Carne blanca asada (`Item::CookedWhiteMeat`) — **falta el PNG** |
| `bottle-empty.png` | Botella vacía |
| `bottle-water.png` | Botella de agua |
| `wheat_seeds.png` | Semillas |
| `wheat.png` | Trigo (icono; el cultivo usa `../blocks/wheat.png`) |
| `potato.png` | Papa |

Sin textura todavía (usan color plano): flecha, y los bloques colocables **Mesa
de trabajo / Cofre / Horno / Molino manual / Antorcha**. La caña usa
`../tools/rudimentary_fishingrod.png`.

Los demás bloques como objeto usan su **cara lateral** (`block_side_sprite` en
`src/item.rs`): PNG único para piedra/arena/grava/madera/nieve/cristal/roca
madre/tierra arada/hojas, y un recorte 16×16 de `../blocks/grass-spritesheet.png`
(césped = tile 0, tierra = tile 2) o `../blocks/wood-spritesheet.png` (tronco =
tile 0).

Pendiente: falta `white_meat-cooked.png` (carne blanca asada); hasta que se añada,
esa casilla carga un asset inexistente y sale un error en el log.

Convención: `<nombre>.png`, PNG con transparencia, 32×32 recomendado.
