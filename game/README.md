# Saffron — supervivencia 2.5D (Bevy + Rust)

Mundo voxel infinito generado proceduralmente, visto en perspectiva de águila
con cámara ortográfica. Inspiración: exploración infinita de Minecraft +
profundidad por capas de Dwarf Fortress.

📖 **[RECIPES.md](RECIPES.md)** — todos los crafteos, fundidos y recolección.

## Menú y guardado

Al arrancar aparece el **menú inicial** (`GameFlow::Menu`, en `menu.rs`) con 4
botones:

- **Jugar** → listado de mundos. Cada `*.json` de `game/saves/` es un mundo;
  eliges uno para cargarlo o **＋ Nuevo mundo** (escribes un nombre → se crea
  `game/saves/<nombre>.json`). `✕` borra un mundo (dos clics: se pone rojo, luego
  confirma). Un `save.json` heredado se migra a `saves/Mundo.json` al arrancar.
- **Multijugador** → listado de servidores, vacío al principio. **＋ Añadir
  servidor** abre un campo de texto (respeta la distribución del teclado, acepta
  **Ctrl+V** para pegar); si omites el puerto se asume `:25599`
  (`net::normalize_addr`). Se guarda en `game/servers.json`. Clic en una fila
  para conectarte; el estado ("Conectando…", "No se pudo conectar: …") aparece
  abajo. `✕` con doble clic borra la entrada.
- **Configuración** → *Skin* (`skins.rs`), *Controles* (`keybinds.rs`),
  *Gráficos* (recorte de visión on/off, radio, brillo ambiental → `graphics.json`).
- **Salir** → cierra el juego.

`Esc` en un subpantalla vuelve atrás. Al jugar:

- **F5** (rebindable) guarda el **mundo activo** (`CurrentWorld`).
- El menú de pausa (`Esc`) → **Guardar y salir** guarda antes de cerrar.
- Cerrar la ventana con la X **no** autoguarda todavía (usa F5).

El guardado (`save.rs`) persiste: semilla del mundo, el **overlay de ediciones**
(`ChunkWorld::edits`, todo lo que has puesto/roto — que además ahora **sobrevive
a la descarga de chunks**), inventario + slot activo, contenido de cofres y
hornos, y posición del jugador. Cada mundo en su propio archivo bajo `saves/`.

## Multijugador (`net.rs`) — v1

TCP casero, sin dependencias. **Qué se sincroniza:** la semilla del mundo, el
overlay de ediciones de bloques (construir juntos), la posición/rotación de cada
jugador, y el chat. **Qué todavía no:** inventario, cofres/hornos, animales,
items tirados, pesca.

- **Unirse**: menú **Multijugador** → añade `IP:puerto` y entra. Se guardan en
  `game/servers.json`. También `game --connect <host:puerto>` desde consola.
- **Modo escucha (Host)**: el botón del menú se retiró; la maquinaria
  `NetMode::Host` sigue en el código para reactivarla más adelante.
- **Modo servidor** (dedicado, tipo Minecraft): `game --server`. Config en
  `game/server.json` (`port`, `seed`, `motd`); el mundo se guarda solo cada 30 s
  en `game/server_world.json` y se recarga al reiniciar. No hay jugador local.
- **Chat**: `Intro` abre la línea de escritura, `Intro` envía, `Esc` cancela.

## Skins (`skins.rs`)

Botón **Skin** en Configuración y en el menú de pausa. Muestra una vista previa y
permite recorrer los `*.png` de `assets/textures/player_skin/` con ◀ ▶. La
elección se guarda en `game/settings.json` y la llevan tanto tu modelo como los
avatares de los demás jugadores.

## Discord Rich Presence (`discord.rs`)

El estado del jugador aparece en tu perfil de Discord. Corre en un hilo aparte
con IPC; si Discord no está abierto no pasa nada (reintenta cada 15 s).

- **Activarlo**: crea una app en <https://discord.com/developers/applications>,
  copia el *Application ID* y ponlo en la variable `SAFFRON_DISCORD_APP_ID` o en
  `game/discord.json` → `{ "app_id": "…" }`. Sin id, la función queda desactivada
  (lo dice en el log al arrancar). Sube arte llamado `logo`, `day` y `night` en
  *Rich Presence → Art Assets*.
- **Qué muestra**:
  - *Menú*: "En el menú" / "Preparando la aventura".
  - *Un jugador*: `Mundo: <nombre>` + `❤ vida  🍖 hambre  💧 sed` (o "☠ Derrotado"),
    icono pequeño día/noche con la fase y la hora, y cronómetro desde que entraste.
  - *Multijugador*: `En línea · <ip:puerto>`, nº de jugadores (party), stats,
    icono día/noche y cronómetro.
- Se refresca cada ~3 s y se envía a Discord como mucho cada 5 s (límite de
  Discord); solo se re-publica si algo cambió.

## Estado: Hito 1 — Mundo + exploración

- Generación procedural por *chunks* columna (`32 × 32 × 128`) con ruido FBm
  (`worldgen.rs`, función pura → corre en el pool de tareas):
  - **Terreno base** rolling + biomas (llanura, bosque, desierto, nieve), mar
    a `y = 48`, árboles.
  - **Montañas**: regiones raras (`mtn_mask`) con crestas afiladas (ruido
    *ridged*), roca bajo la superficie a partir de `y ≥ 84` y **picos nevados**
    por encima de una cota de nieve con ruido (`~82–94`), con roca desnuda
    asomando en los picos más altos.
  - **Ríos**: donde el terreno base es bajo (`< mar + 12`), los cruces por cero
    del ruido `river` cavan un cauce con agua y lecho de arena.
  - **Cuevas**: túneles 3D (intersección de dos campos de ruido) + cavernas
    tipo *blob* más frecuentes cerca de la roca madre. No se cavan bajo el
    fondo marino costero. *(Aún sin iluminación de cueva — el juego no tiene luz
    por voxel.)*
  - **Grietas / ravinas**: cortes verticales estrechos y profundos que rompen
    la superficie, en regiones dispersas (`ravine_mask`).
- **Cutout de cámara**: el material de los chunks es un `ExtendedMaterial` con
  un shader que vuelve translúcidos (alpha-to-coverage) los fragmentos que están
  entre la cámara y el jugador dentro de un radio en pantalla. El jugador nunca
  queda tapado por salientes, árboles o techos. Se activa/desactiva con `K` y se
  ajusta en `CutoutSettings` (`radius`, `feather`, `min_alpha`). El **raycast de
  interacción atraviesa** los bloques que el cutout está ocultando, así puedes
  minar/apuntar a lo que ves detrás de un saliente translúcido.
- **Capas de visión** (estilo Dwarf Fortress): oculta los voxels por encima de
  un corte en Y para que la cámara de águila nunca pierda de vista al jugador.
  - `L` activa/desactiva el modo **automático**: el corte sigue al jugador
    (justo sobre su cabeza) fotograma a fotograma.
  - `[` / `]` ajustan el corte (en auto, lo desplazan arriba/abajo; si está
    apagado, pasan a un corte manual fijo). `\` vuelve a vista completa.
  - Al cambiar el corte, los chunks cargados se re-mallean empezando por los
    más cercanos al jugador (`ChunkSlot::meshed_at` vs. corte actual).
- *Streaming* infinito: generación y *meshing* en `AsyncComputeTaskPool`,
  carga/descarga de chunks alrededor del jugador.
- *Meshing* por descarte de caras con sombreado por cara horneado en los
  vértices (da sensación de relieve sin luz costosa). El **agua** se mallea en un
  buffer aparte y se dibuja con un material translúcido (`AlphaMode::Blend`).
- **Texturas de bloques**: `block_atlas.rs` monta en runtime un atlas 240×16 (15
  columnas) con los PNG de `textures/blocks/` (césped lado/arriba/abajo, madera
  lado/tapa, hojas, arena, roca, grava, tablas, tierra arada, nieve, cristal,
  roca madre) y lo enchufa al material del chunk. El resto de bloques siguen con
  color plano. En el inventario, un bloque como objeto muestra su **cara
  lateral** (`Item::inventory_sprite` / `block_side_sprite`). El **cristal** es
  translúcido: se mallea en un buffer aparte (`MeshData::glass`) y usa un
  material `AlphaMode::Blend` con el mismo atlas y shader de recorte
  (`GlassMaterialHandle`).
- **Modelos GLB**: `props.rs` dibuja **banco / cofre / horno** como su modelo
  glTF en vez de un cubo (el bloque sigue existiendo para colisión y raycast).
  La **tapa del cofre** se anima al abrir/cerrar; el **fuego del horno**
  (nodo `sphere`) se enciende cuando está fundiendo.
- **Peces** que nadan dentro del agua (visibles ahora que es translúcida) y se
  pueden pescar con la caña; al capturar desaparece el pez más cercano.
- Cámara ortográfica cenital que sigue al jugador, con zoom (Ctrl + rueda) y
  rotación de 90° (Q / E).
- Movimiento **point & click** estilo League of Legends / Diablo: clic derecho
  para caminar hacia el punto del suelo (raycast DDA contra voxels), con
  subida automática de escalones de 1 bloque y cancelación si se queda atascado.
- Jugador con gravedad y colisión AABB contra voxels; **salto** con la barra
  espaciadora (subida automática de escalones de 1 bloque para el resto).
- **Modelo 3D del jugador** (`models/player/Player.glb`) con skin intercambiable
  desde `textures/player_skin/` y ciclo de caminar que se reproduce solo al
  moverse (al detenerse vuelve al primer fotograma). Sombreado básico: material
  mate iluminado por la escena + una **sombra de contacto** (disco oscuro que se
  pega al suelo bajo el jugador y se desvanece al saltar/volar). La caña y las
  **herramientas de pedernal** (cuchillo, hacha, pico, pala, hoz) usan sus modelos 3D
  en la mano (`Item::hand_model`), anclados al hueso `right_arm`; el resto de
  items van en un cubo con su textura.
- **Recolección**: con la mano vacía / herramienta, **mantén** clic izquierdo
  para minar; tarda según el material y la herramienta, con barrita en el cursor,
  outline que se encoge y escombros. **Talar** derriba solo el **tronco vertical**
  clicado + su copa cercana (detecta que es un árbol real por la forma: tronco
  corto rematado en hojas). La madera colocada por el jugador se rompe bloque a
  bloque y los árboles vecinos conservan su tronco. Las hojas no dan nada.
  **Palos y plantas** en el suelo se recogen al pasar por encima.
- **Herramientas obligatorias**: no se puede talar madera sin **hacha**, ni picar
  **piedra** sin **pico**, ni **tierra/hierba** sin **pala o pico**. La grava,
  arena, nieve y hojas se rompen a mano. Romper **grava** da **pedernal** (~35 %)
  o grava.
- **Crafteo por formas** (`I`, tipo Minecraft): coloca los objetos en la
  cuadrícula **2×2** (clic izq coge/deja pila, clic der deja 1 / coge la mitad)
  formando la **forma** de la receta (la posición relativa importa, y su reflejo
  también vale) y haz clic en el resultado. La única receta sin forma es la Soga
  (3 Fibra en cualquier casilla). **Los Troncos** (talar árboles) se refinan en
  **Madera** (1 Tronco → 4 Madera), que es lo que piden el Banco y el Cofre.
  Recetas de mano: Madera ×4 = 1 Tronco · Palos ×4 = 2 Troncos (columna) ·
  Cuchillo = Pedernal sobre Palo · 4 Flechas = Pedernal + Palo ·
  **Banco de trabajo = 4 Madera (2×2)** · Hacha = 2 Pedernal arriba + Palo/Pedernal.
- **Estaciones con `W`**: pulsa `W` para usar una estación cercana (radio 4).
  Si solo hay una, se abre directa; si hay varias, aparece un **selector** para
  elegir. `W` de nuevo (o `Esc`) cierra. Ya no es por proximidad.
  - **Banco de trabajo** → panel `I` con cuadrícula **3×3**: Pico, Pala y Caña
    (Pedernal + Palo / Soga / Fibra) · **Cofre = 8 Madera** · **Horno = 8 Piedra**.
    (El Hacha es la única herramienta del 2×2.) Detalle completo en `RECIPES.md`.
  - **Cofre / Horno** → su panel (también se pueden abrir con clic izquierdo).
- **Cofres**: clic izquierdo (con la mano vacía / no un bloque) para abrir. 27
  ranuras; dos cofres pegados forman un **cofre doble** (54). Shift + mantener
  clic izquierdo para romperlo (te devuelve el contenido).
- **Horno**: 3 ranuras (mineral / combustible / resultado). Funde en segundo
  plano: Pescado → Pescado cocinado, Arena → Cristal. Combustible: palos, madera,
  hojas. Muestra % de fundido y combustible restante.
- **Caña de pescar** (lanzar y recoger, sin minijuego): con la caña seleccionada,
  clic izquierdo sobre agua lanza el flotador. Tras una espera corta el pez pica
  (el flotador se hunde y sale el aviso "¡Pica!"); **clic izquierdo otra vez**
  para recogerlo y guardar el Pescado. Si recoges antes de que pique, sacas la
  línea vacía.
- **Inventario 5×10** (`I`): fila 0 = hotbar. Hotbar seleccionable con `1`..`0`
  (fila y numpad) o la **rueda del ratón**. Tooltip de nombre al pasar el ratón;
  fuera del inventario el nombre del objeto aparece sobre la hotbar ~2 s al
  cambiar de slot. Los items y herramientas con textura muestran su sprite en
  las casillas; el objeto seleccionado aparece **en la mano** del jugador (cubo
  con la textura del item, o el modelo 3D en el caso de la caña).
- **Animales**: vacas, pollos, ovejas y cerdos aparecen en **manadas de 3** por
  chunk y deambulan siguiendo el terreno. **Clic izquierdo cerca** de un animal
  lo golpea (mano = 2, cuchillo/hacha = 5, con empujón). Al morir suelta comida
  y recursos en el suelo:
  | Animal | Suelta |
  |---|---|
  | Cerdo | Carne + Grasa |
  | Vaca | Carne roja + Cuero |
  | Oveja | Cordero + Lana |
  | Pollo | Carne blanca + Pluma |
  Las carnes crudas se comen (`G`) o se funden en su asado por especie —
  Carne asada / Carne roja asada / Cordero asado / Carne blanca asada (más
  saciante, +30 hambre).
  La Grasa sirve de combustible.
  **Cebo**: si llevas en la mano la comida favorita de un animal (Semillas →
  gallinas, Trigo → vacas y ovejas, Papa → cerdos) lo **atraes** (te sigue), y
  golpearlo con esa comida lo **alimenta** (cura a tope, consume 1) en vez de
  herirlo.
  **Cría** (`breed_animals`): alimentar a un adulto lo pone en *modo amor* ~18 s;
  dos animales del mismo tipo en modo amor y juntos (≤2,2) generan una **cría**,
  y luego no pueden volver a criar durante 75 s. La cría nace pequeña y crece en
  ~75 s (alimentarla acelera el crecimiento −12 s); las crías no sueltan nada al
  morir. Tope de 12 animales por chunk de origen. *(Los animales no se guardan:
  las crías desaparecen al descargar el chunk, como el resto.)*
- **Ciclo día/noche** (`daynight.rs`): un día completo dura 20 min reales. El sol
  gira y cambia de color, el cielo y la luz ambiental pasan de día → atardecer →
  noche → amanecer. De noche baja la luz (con un tinte azul tenue de "luna"). La
  hora se ve en el HUD y se guarda en la partida.
- **Antorchas**: se craftean con Palo + Soga + Carbón vegetal (×4). Al colocarlas
  emiten luz cálida (`PointLight`); se dibujan como un prop pequeño (palito +
  llama), se atraviesan y se rompen de un golpe.
- **Vida, hambre y sed** (`survival.rs`): la barra de **Vida** va sobre el
  extremo **izquierdo** de la hotbar; **Hambre** y **Sed** sobre el **derecho**.
  El **hambre** sólo baja *al hacer algo* — caminar/correr, o trabajar el molino
  manual — nunca en reposo. La **sed** baja siempre (12:30 de 100 a 0 sin correr,
  más rápido corriendo). A 0, cualquiera de las dos empieza a quitar **vida**
  (lento). Con hambre y sed por encima de 60 % la vida se regenera sola. **`G`** come el item seleccionado si es comida; si no llevas
  comida y estás **junto a agua** (orilla, vadeando o nadando), `G` bebe (+28
  sed) o **llena una botella vacía** (→ Botella de agua). Beber una **Botella de
  agua** (`G`) da +45 sed y te devuelve la botella vacía. Si la vida llega a 0
  reapareces en tu punto de inicio con todo lleno (conservas el inventario).
- **Botellas**: 2 vidrios uno encima del otro → 2 botellas vacías (a mano).
- **Agricultura** (`farming.rs`):
  - **Hoz de pedernal** — *sólo* ara: clic izquierdo sobre Tierra/Hierba la
    convierte en **Tierra arada**.
  - La tierra arada **se seca y vuelve a tierra normal** a los ~45 s si no está
    encadenada (a través de otras tierras aradas, hasta 3 saltos) a un bloque de
    **agua**. La tierra hidratada además hace crecer el trigo 1,5× más rápido.
  - **Semillas**: se consiguen al cortar plantas silvestres con el cuchillo
    (~40 %). Clic izquierdo con Semillas sobre tierra arada (con aire encima) →
    planta **Trigo** (tarda ~3 min en madurar). Romper la planta madura da Trigo
    (+ 50 % de una semilla); si la rompes verde recuperas la semilla. El trigo se
    dibuja como **planos en cruz** con textura (`textures/blocks/wheat.png`), igual
    que el pasto silvestre (`tall_grass.png`) y la papa silvestre (`potatoes.png`);
    crece en altura y (con un tinte) se dora al madurar. La tierra arada usa
    `plowed_land.png` en el atlas de voxels.
  - **Papas**: aparecen sueltas en el mundo (silvestres), se recogen al pasar.
  - **Molino manual**: bloque crafteado, modelo 3D (`ManualMill.glb`). Clic
    izquierdo lo abre (como
    cofre/horno): ranura **[1] a moler** → botón **MOLER (mantener)** → ranura
    **[2] resultado**. Mantener el botón muele Trigo → **Harina** y gasta hambre
    mientras dura. Harina + Botella de agua → **Masa** (a mano; la botella vuelve
    vacía). Masa fundida en el horno → **Pan** (+35 hambre).
- **Menú de pausa** (`Esc`): congela el tiempo virtual y muestra
  *Volver al juego* / *Salir del juego*.
- **Construcción**: con un bloque en la mano, clic izquierdo lo coloca contra la
  cara apuntada. Modo estancia (`B`): dos clics definen un rectángulo y se
  levanta suelo + muros (altura fija de 4), consumiendo bloques del slot activo.
- Resaltado del bloque objetivo con gizmo (rojo romper, azul colocar, naranja
  estancia).

## Controles

Las teclas de juego son **remapeables** en Configuración → Controles
(`keybinds.rs`, `Action`), y se guardan en `game/keybinds.json`. Quedan fijas:
`Esc` (menús), `1`‥`0` / numpad (hotbar), `Ctrl` (zoom / bajar al volar) y
`Shift` como paso grande de la rebanada. Valores por defecto:

| Entrada | Acción |
|---------|--------|
| Clic derecho (mantener) | Mover hacia el punto del suelo |
| Clic izquierdo (mantener, mano vacía) | Minar / talar el bloque apuntado |
| Clic izquierdo (con bloque en mano) | Colocar bloque contra la cara apuntada |
| `1` .. `0` (fila o numpad) | Seleccionar slot del hotbar |
| Rueda | Ciclar el slot del hotbar |
| Ctrl + rueda | Zoom de cámara |
| `I` | Abrir/cerrar inventario 5×10 y panel de crafteo (2×2) |
| `W` | Usar estación cercana (banco / cofre / horno); selector si hay varias |
| `B` | Modo estancia (2 clics: esquinas → suelo + muros) |
| Barra espaciadora | Saltar |
| `G` | Comer / beber junto al agua / llenar botella vacía junto al agua |
| Clic izq. cerca de un animal | Golpearlo (o alimentarlo si llevas su comida) |
| Clic izq. con **Hoz** sobre tierra | Arar |
| Clic izq. con **Semillas** sobre tierra arada | Plantar trigo |
| Clic izq. sobre **Molino manual** | Abrir su panel (mantener **MOLER** para moler) |
| Shift | Correr (o paso grande al cambiar capa) |
| `Esc` | Cierra el menú abierto (inventario / cofre / pesca); si no hay ninguno, abre el menú de pausa |
| `K` | Cutout de cámara (bloques que tapan al jugador → translúcidos) on/off |
| `L` | Capa de visión automática (sigue al jugador) on/off |
| `[` / `]` | Ajustar el corte de la capa |
| `\` | Vista completa (quitar la capa) |
| Q / E | Rotar cámara 90° |
| Rueda | Zoom |
| Esc | Salir |

## Ejecutar

```bash
cargo run
```

Iteración más rápida (linkado dinámico de Bevy):

```bash
cargo run --features dev
```

Servidor dedicado / unirse directo:

```bash
cargo run -- --server
cargo run -- --connect 127.0.0.1:25599
```

## Estructura

| Archivo | Responsabilidad |
|---------|-----------------|
| `block.rs` | Tipos de voxel y sus propiedades (color, opacidad, colisión) |
| `chunk.rs` | Almacenamiento de chunk y helpers de coordenadas |
| `worldgen.rs` | Generación procedural (ruido → `ChunkData`) |
| `mesher.rs` | `ChunkData` → malla por descarte de caras (con corte de capa `max_y`, UVs de atlas) |
| `chunk_material.rs` | Material de chunk (`ExtendedMaterial` + shader de cutout) + material de agua |
| `block_atlas.rs` | Monta el atlas de texturas de bloque en runtime |
| `props.rs` | Modelos glTF de banco / cofre / horno sobre los bloques |
| `streaming.rs` | Carga/descarga infinita, tareas en segundo plano, re-mallado por capa, `set_block` |
| `player.rs` | Spawn, movimiento point & click y colisión del jugador |
| `camera.rs` | Cámara ortográfica de seguimiento |
| `view.rs` | Capa de visión (corte en Y) y sus controles |
| `item.rs` | Items, inventario 5×10, hotbar, herramientas, crafteo (UI) |
| `interact.rs` | Raycast a celda, minado temporizado, talar/colocar, estancias |
| `scatter.rs` | Palos y plantas en el suelo (generación y recogida) |
| `animal.rs` | Manadas de animales que deambulan, se golpean y sueltan comida/recursos |
| `daynight.rs` | Ciclo día/noche: sol, cielo y luz ambiental según `GameClock` |
| `survival.rs` | Vida / hambre / sed, comer-beber (`G`), muerte y reaparición |
| `farming.rs` | Arado (hoz), hidratación de tierra arada, siembra y crecimiento del trigo |
| `save.rs` | Menú inicial (`GameFlow` state) + guardado/carga JSON |
| `container.rs` | Cofres (simple/doble), horno y molino manual: almacenamiento, fundido/molienda, UI |
| `fishing.rs` | Caña, lanzamiento sobre agua y minijuego de pesca |
| `station.rs` | Tecla `W`: escaneo + selector de estación (banco / cofre / horno) |
| `pause.rs` | Menú de pausa (`Esc`) y condición `not_paused` |
| `net.rs` | Multijugador TCP casero: modos Host/Client/Server, sync de ediciones + jugadores + chat |
| `menu.rs` | Front-end: Jugar (mundos) / Multijugador (servidores) / Configuración / Salir |
| `keybinds.rs` | Controles remapeables (`Action` → `KeyCode`) + pantalla de rebind |
| `skins.rs` | Catálogo de skins + pantalla para verlas y elegirlas |
| `discord.rs` | Discord Rich Presence en hilo aparte (estado del jugador) |
| `hud.rs` | Lectura de depuración en pantalla |

## Assets (para el futuro)

- `assets/textures/blocks/` — se montan en un atlas en runtime (`block_atlas.rs`).
- `assets/textures/tools|items/` — iconos de inventario y modelo en mano
  (`Item::texture_path` + `item::paint_icon` / `player::held_look`). Los que
  faltan caen a color plano.
- `assets/models/` — modelos 3D `.glb` de cosas especiales (cofre, horno, banco…).
- `assets/shaders/chunk_cutout.wgsl` — shader del material de chunk (en uso).
- `assets/icon.ico` — icono del juego. En runtime se incrusta con `include_bytes!`
  y se aplica a la ventana (`main::set_window_icon`); en Windows `build.rs`
  (`winresource`) lo mete además en el `.exe` (Explorer / barra de tareas). En
  Linux `build.rs` no hace nada; si falta `rc.exe` del SDK de Windows el build
  sigue con un warning.

## Siguientes hitos (propuesta)

1. Mover también el corte de capa (`view.rs`) al shader del material
   (`chunk_material.rs` ya establece el patrón) para quitar el re-mallado.
2. Sub-chunks verticales + *greedy meshing* para vistas más lejanas.
3. Editar bloques (minar/colocar) con re-mesh incremental.
4. Agua translúcida en pasada separada; niebla de distancia.
5. Inventario, recursos y bucle de supervivencia (hambre/salud).
6. Guardado del mundo en disco.
