# Modelos 3D de objetos especiales

Modelos con geometría propia (no un simple cubo de voxel) para cosas destacadas
del mundo: cofre, horno, banco de trabajo, futuros muebles/estaciones, la caña de
pescar equipada, el flotador, animales con más detalle, etc.

Formato: **glTF binario** (`.glb`) o `.gltf` + assets. Bevy lo carga con
`asset_server.load("models/<archivo>.glb#Scene0")`.

Escala: 1 unidad = 1 bloque. Origen en la base del modelo, mirando hacia -Z.

**En uso.** `src/props.rs::model_path` mapea cada bloque-prop a su `.glb` y
`sync_props` spawnea `WorldAssetRoot(load("<archivo>.glb#Scene0"))` sobre él
(el `mesher` ya no emite el cubo para esos bloques). Props con GLB:
`WorkstationT1` (banco), `ChestT1` (cofre), `FurnaceT1` (horno),
**`ManualMill` (molino manual — textura embebida; trae un clip de animación aún
sin usar)**. Ajustes en `props.rs`: `PROP_SCALE`, `PROP_Y`, `LID_MAX_RAD`.

- **Cofre**: se busca el nodo `chest_lid` y se rota (`Quat::from_rotation_x`)
  entre cerrado y abierto según si es el contenedor abierto. No se usan los
  clips `chest_t1_*-animation` del GLB (rotación manual, más robusto).
- **Horno**: se busca el nodo `sphere` y se muestra/oculta según `burn > 0`.
  `furnace_t1_fire-texture.png` (cuerpo encendido) aún no está enchufada.
- **Molino manual**: modelo estático por ahora (el clip del crank no se
  reproduce). `attach_player_anim` (en `player.rs`) filtra por descendencia de
  `PlayerModel` para no enganchar el walk-graph al `AnimationPlayer` del molino.

Las **plantas silvestres y los cultivos** (trigo) NO usan GLB: son **planos en
cruz** procedurales (`mesher::crossed_quads`, doble cara, `AlphaMode::Mask`) —
listos para texturas en cruz estilo Minecraft cuando lleguen.

## Jugador y caña (`src/player.rs`)

- **`player/Player.glb`** — modelo del jugador (Blockbench, ~0.69 u de alto,
  origen en los pies, mirando a -Z, textura embebida). Se spawnea como hijo de la
  entidad `Player` con `PLAYER_MODEL_SCALE = 2.6` y un giro de 180° para mirar en
  la dirección de avance. `apply_player_skin` recorre sus mallas y les pone
  `textures/player_skin/motamore_skin.png` (skin intercambiable: cambia
  `PLAYER_SKIN`). Trae un clip `walk-animation`: `setup_player_assets` monta un
  `AnimationGraph`, `attach_player_anim` lo engancha al `AnimationPlayer` que Bevy
  añade a la escena, y `drive_player_anim` lo reproduce solo mientras el jugador
  tiene velocidad horizontal.
- **Ancla de la mano**: `attach_held_to_arm` busca el nodo `right_arm` del
  modelo ya spawneado y re-parenta ahí el ancla `HeldItem` (con `HAND_LOCAL` en
  la base del brazo y escala `1/PLAYER_MODEL_SCALE` para cancelar la del modelo),
  así lo que llevas en la mano acompaña el balanceo del brazo al caminar.
- **Modelos en la mano** (`Item::hand_model` + `player::hand_model_transform`):
  `RudimentaryFishingrod.glb` (caña) y las herramientas de pedernal
  `Flint{Knife,Axe,Pickaxe,Shovel,Sickle}.glb` (textura embebida). Se spawnea uno
  por item como hijo del ancla `HeldItem` con un `HeldModel(Item)`;
  `update_held_item` hace visible el que coincide con el slot seleccionado y
  oculta el cubo `HeldCube`. La pose (escala + rotación + offset) de cada uno se
  ajusta en `player::hand_model_transform`.

