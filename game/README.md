# Saffron — supervivencia 2.5D (Bevy + Rust)

Mundo voxel infinito generado proceduralmente, visto en perspectiva de águila
con cámara ortográfica. Inspiración: exploración infinita de Minecraft +
profundidad por capas de Dwarf Fortress.

📖 **[RECIPES.md](RECIPES.md)** — todos los crafteos, fundidos y recolección.
🧱 **[STRUCTURES.md](STRUCTURES.md)** — el editor de estructuras y el formato `.json` para compartir/implementar builds.

## Menú y guardado

Al arrancar aparece el **menú inicial** (`GameFlow::Menu`, en `menu.rs`) con 5
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
- **Structure Editor** → editor de estructuras aparte (`editor.rs`). Ver abajo.
- **Configuración** → *Skin* (`skins.rs`), *Controles* (`keybinds.rs`),
  *Gráficos* → `graphics.json`: recorte de visión (on/off, radio), brillo
  ambiental, **límite de FPS** (`0` = ilimitado · 30 · 60 · 120 · 144; `limit_fps`
  duerme al final del frame) y **Ray tracing** (`advanced_shading`): añade **SSAO**
  a la cámara + **sombras de 4096** — *no es RT por hardware* (esta versión del
  motor no lo trae), es el salto de calidad más cercano sin él. SSAO exige
  `Msaa::Off`, y el **recorte de visión** usa alpha-to-coverage MSAA, así que
  activar Ray tracing **desactiva el recorte** mientras esté puesto.
- **Salir** → cierra el juego.

`Esc` en un subpantalla vuelve atrás. Al jugar:

- **F5** (rebindable) guarda el **mundo activo** (`CurrentWorld`).
- El menú de pausa (`Esc`) → **Guardar y salir** guarda antes de cerrar.
- Cerrar la ventana con la X **no** autoguarda todavía (usa F5).

El guardado (`save.rs`) persiste: semilla del mundo, el **overlay de ediciones**
(`ChunkWorld::edits`, todo lo que has puesto/roto — que además ahora **sobrevive
a la descarga de chunks**), inventario + slot activo, contenido de cofres y
hornos, y posición del jugador. Cada mundo en su propio archivo bajo `saves/`.

## Chat y comandos (`command.rs`)

El chat funciona **también en un jugador**, no solo en red. `Intro` abre la línea
de escritura, `Intro` envía, `Esc` cancela (mientras escribes, el jugador no se
mueve). En red, el texto normal viaja a los demás; en un jugador solo se muestra
en tu pantalla.

Las líneas que empiezan por `/` son **comandos** (nunca se envían por la red) y
los procesa `command::run_commands`. Van en inglés, como el texto in-game:

| Comando | Qué hace |
|---|---|
| `/help` | lista los comandos |
| `/pos` | muestra tus coordenadas |
| `/biome <name>` | te teletransporta al bioma más cercano — `plains`, `forest`, `desert`, `snow` |
| `/structure` | a la estructura de librería generable más cercana (necesita `.json` en `assets/structures/`) |
| `/city` | a la ciudad en ruinas más cercana |
| `/spawn` | de vuelta al punto de aparición del mundo |

Las búsquedas (`worldgen::nearest_biome` / `nearest_structure` /
`nearest_ruined_city`) recorren en anillos crecientes desde tu posición y son
deterministas. El teletransporte **siempre aterriza sobre la superficie**
(`command::safe_landing` escanea la columna real del destino —generándola si el
chunk aún no está cargado— para dejarte encima del bloque sólido más alto, nunca
dentro de un edificio o del terreno), con la velocidad a cero.

## Multijugador (`net.rs`) — v1

TCP casero, sin dependencias. **Qué se sincroniza:** la semilla del mundo, el
overlay de ediciones de bloques (construir juntos), la posición/rotación de cada
jugador, y el chat. **Qué todavía no se sincroniza en vivo:** cofres/hornos,
animales, items tirados, pesca.

- **Unirse**: menú **Multijugador** → añade `IP:puerto` y entra. Se guardan en
  `game/servers.json`. También `game --connect <host:puerto>` desde consola.
- **Modo escucha (Host)**: el botón del menú se retiró; la maquinaria
  `NetMode::Host` sigue en el código para reactivarla más adelante.
- **Modo servidor** (dedicado, tipo Minecraft): `game --server`. Config en
  `game/server.json` (`port`, `seed`, `motd`); todo se guarda en
  `game/server_world.json` y se recarga al reiniciar. No hay jugador local.
- **Guardado del jugador en el servidor** (`ClientMsg::SaveState` /
  `ServerMsg::Welcome`, `PlayerRecord`): el cliente envía su **inventario +
  vida/hambre/sed + posición** al servidor cada 10 s, con F5, y al *Guardar y
  salir* (con un margen de ~8 frames para vaciar el socket). El servidor los
  guarda por **nombre de jugador** (`whoami()` → `USERNAME`/`USER`) en
  `server_world.json` (autosave 30 s + al desconectarse cada jugador). Al volver
  a conectarte con el mismo nombre te devuelve tu inventario, stats y posición.
  *Limitación:* dos jugadores con el mismo nombre de usuario comparten ranura.
  En modo cliente **no** se escribe `saves/*.json` local: manda el servidor.
- **Chat**: ver la sección *Chat y comandos* más arriba (funciona en red y en un
  jugador; `/` = comando local).

## Skins (`skins.rs`)

Botón **Skin** en Configuración y en el menú de pausa. Muestra una vista previa y
permite recorrer los `*.png` de `assets/textures/player_skin/` con ◀ ▶. La
elección se guarda en `game/settings.json` y la llevan tanto tu modelo como los
avatares de los demás jugadores.

## Structure Editor (`editor.rs`)

Estado propio (`GameFlow::Editor`, botón **Structure Editor** en el menú) — **no
es** la partida: sin jugador, sin supervivencia, sin hotbar. Cámara y HUD
propios, para diseñar estructuras y exportarlas.

- **Cámara libre 3D**: clic derecho para mirar, **WASD** volar,
  **Espacio/Ctrl** subir/bajar, **Shift** más rápido, rueda = velocidad.
- **Content browser** (panel izquierdo): un swatch por bloque y por prop con
  modelo (banco, cofre, horno, molino, antorcha) + brocha **Erase**.
- **Herramienta Build**: clic izquierdo coloca el bloque de la brocha en la cara
  apuntada (o a 8 m en el aire); **Alt+clic** o la brocha *Erase* lo quita.
- **Herramienta Select**: clic izquierdo elige un bloque · **Shift+clic** lo
  añade/quita del grupo · **Ctrl+clic** selecciona **todo**. **Flechas** y
  **RePág/AvPág** mueven la selección una celda; **Supr** la borra. Botones
  `Select all` / `Clear sel` / `Delete`.
- **Textured**: alterna entre la textura real del atlas y color plano (solo
  bloques normales; los props siempre muestran su GLB). La preview con textura
  es aproximada — el juego base la coloca bien al estampar.
- **Save file** → `game/structures/<nombre>.json` · **Copy/Paste JSON**
  (portapapeles) · lista de `structures/*.json` (clic → cargar y seguir
  editando). `Rename` para el nombre. `Esc` vuelve al menú.
- Los props con modelo se cargan de verdad (GLB); los bloques se dibujan con
  color plano (la textura real la pone el juego base al estampar).
- **Regla de aparición** (`spawn` en el `.json`, controles `w±` / `sink±` en el
  panel): `weight` (0 = no aparece), `min_y`/`max_y`, `max_slope`, `sink`,
  `clear`, `fill_below`.
- **Devs**: `structure::{load_structure, stamp_structure}` (y `Structure`,
  `from_cells`) son públicas. Formato en **[STRUCTURES.md](STRUCTURES.md)**.

### Aparición en la generación del mundo

`structure::StructureLibraryPlugin` escanea `game/assets/structures/*.json` +
`game/structures/*.json` al arrancar y mete un `StructureLibrary` (`Arc<Library>`)
que se pasa a `WorldGen`. En `worldgen::generate`, `stamp_structures` divide el
mundo en **regiones de 4×4 chunks**; cada región tira un dado determinista
(semilla + coords, `STRUCT_PER_MIL ≈ 13 %`), elige una estructura por peso, la
ancla dentro de la región (nunca cruza a otra), comprueba pendiente/altura y la
estampa por rebanadas en cada chunk. **Todo es determinista desde la semilla** →
cada cliente y cada recarga generan lo mismo, sin red ni guardado. Las
modificaciones del jugador se guardan encima (`ChunkWorld.edits`) como con el
terreno. Para que una estructura aparezca: `spawn.weight > 0` y ponla en
`game/assets/structures/` (commiteada; en multijugador todas las máquinas deben
tener los mismos archivos).

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
  - **Arcilla / Barro**: parches de Arcilla en la arena de bajíos y playas de
    lagos/ríos; Barro en las riberas de hierba pegadas a un río sin arena.
  - **Agua contaminada** (`WorldGen::water_at`, ruido `contam`): una fracción de
    los **ríos** lleva **Agua Irradiada** y algunas **charcas interiores**
    (agua estancada bajo el nivel del mar) llevan **Agua Tóxica**. Determinista;
    la mayor parte del agua sigue siendo limpia (transición gradual).
  - **Ciudades en ruinas** (`stamp_ruins`): en regiones de `16 × 16` chunks
    (`CITY_PER_MIL ≈ 9 %`), sobre tierra firme. La ciudad es una rejilla de
    **manzanas** (`2×2..3×3`) separadas por **calles** (`STREET_W = 9`); cada
    manzana tiene **3–7 bloques de pisos** grandes (`15..22` de lado, `4..12`
    plantas) casi pegados (`BUILD_GAP = 2`). El suelo de la ciudad es **Césped**
    dentro de las manzanas y **caminos de Grava** por las calles entre ellas.
    **Gradiente de material** por
    altura (`ruin_material`): base de **Ladrillos de Roca**, cuerpo de
    **Cemento**, remate de **Ladrillo**; cimientos de Piedra. Interiores
    **estrechos** estilo khrushchyovka: rejilla de tabiques con cuartos de
    `ROOM_STEP = 3` y un pasillo de 1 por el eje mayor. **Techos a medio
    derruir** (agujeros que crecen con el daño y la altura) y una **caja de
    escalera** en una esquina — dos peldaños + hueco en cada losa — para subir
    de planta. Cada edificio deja **0–2 cofres** en la planta baja con **loot**
    determinista (Pastillas purificadoras, Vodka, Anti-Rad Meds, Carbón — o
    vacío; `container::seed_ruin_chests`) y las calles tienen algún **charco de
    agua irradiada**. Medio derrumbados (ruido `ruin`) — inspiración soviética
    temprana / *Samosbor*. Cada edificio se nivela a su propia base y todo se
    ancla dentro de la región → determinista desde la semilla, sin red ni
    guardado, igual que las estructuras.
- **Aparición del jugador**: al empezar un mundo nuevo el jugador nunca cae al
  agua — `WorldGen::find_land` busca en espiral la columna de tierra firme más
  cercana al origen (por encima del mar, fuera de ríos).
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
  outline que se encoge y escombros. **Alcance de minado / colocado: 5 bloques**
  desde el jugador (`INTERACT_REACH`), igual en primera y tercera persona. **Talar** derriba solo el **tronco vertical**
  clicado + su copa cercana (detecta que es un árbol real por la forma: tronco
  corto rematado en hojas). La madera colocada por el jugador se rompe bloque a
  bloque y los árboles vecinos conservan su tronco. Las hojas no dan nada.
  **Palos y plantas** en el suelo se recogen al pasar por encima.
- **Herramientas obligatorias**: no se puede talar madera sin **hacha**, ni picar
  **piedra** sin **pico**, ni **tierra/hierba** sin **pala o pico**. La grava,
  arena, nieve y hojas se rompen a mano. Romper **grava** da **pedernal** (~35 %)
  o grava. Picar **Roca** (`Stone`) da **Piedra** (`Cobblestone`); fundir Piedra
  la devuelve a Roca. Romper **Arcilla** da **Bodoques de arcilla** ×4.
  **Cemento** es un objeto (mortero): clic izq. sobre un bloque de **Ladrillo**
  lo fragua en Cemento; romper ese Cemento sólo devuelve Ladrillos (objeto).
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
    (Pedernal + Palo / Soga / Fibra) · **Cofre = 8 Madera** · **Horno = 8 Piedra**
    (`Cobblestone`). (El Hacha es la única herramienta del 2×2.) Cadena de
    mampostería (Roca Pulida, Ladrillos de Roca, Cemento, bloque de Ladrillo) y
    detalle completo en `RECIPES.md`.
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
- **Radiación e intoxicación** (`survival.rs`, giro post-apocalíptico): dos barras
  más bajo Hambre/Sed que **sólo aparecen al contaminarte**. Suben al **beber
  agua contaminada** cruda o al **nadar en ella**; bajan solas (la radiación muy
  despacio). Por encima de ~45 % (rad) / ~40 % (tox) hacen daño a la vida y
  cortan la regeneración. **Vodka** (3 Papas) baja la intoxicación de golpe;
  **Anti-Rad Meds** (loot de ruinas) baja la radiación.
- **Aviso de zona irradiada** (`radiation.rs`): al acercarse (o entrar) a agua
  irradiada aparece un **granulado verdoso en pantalla** que se intensifica con
  la cercanía, y suena el **crepitar de un contador Geiger** (ticks sintéticos
  cada vez más rápidos). El campo (`RadField.ambient`, 0..1) se muestrea en un
  radio de 8 bloques y se suaviza; también hace *tick* si llevas radiación
  encima aunque no haya fuente cerca. Sólo es feedback — el daño va por `Stats`.
- **Monigote de extremidades** (estilo Fallout, en el panel de inventario): 6
  recuadros colocados como un cuerpo (cabeza / torso / 2 brazos / 2 piernas) que
  se tiñen de **verde→amarillo→rojo** con la vida de cada parte y muestran el %.
  Las extremidades siguen a la salud global con pesos (cabeza y torso aguantan
  más). El **Botiquín** (`Item::Medkit`, loot de cofres de ruinas) cura +55 a la
  salud y a cada parte.
- **Desgaste** (`Stack.wear`, `Item::wear_kind`): las **herramientas** pierden
  **durabilidad** al usarse (romper bloques, talar, arar, pescar, cazar) y se
  rompen a 0; la **comida se pudre** con el tiempo (`tick_spoilage`, 1 pt/s) —
  la cruda ~6 min, la cocinada ~16 min — hasta convertirse en *Rotten Food*
  (intoxica). El tooltip del inventario muestra el estado (`% dur.` / `% fresh`).
  Hay un hook oculto de **oxidación** (`WearKind::Rust`) para futuras armas de
  fuego / equipo metálico. `wear` se guarda en el save y se mezcla al apilar.
- **Tratar el agua**: **Balde** (3 Maderas) + `G` metido en el agua → balde de
  agua *limpia / irradiada / tóxica* según el tipo. El balde crudo se **hierve
  en una Fogata** (panel tipo horno: agua · combustible · resultado) y luego
  `Balde hervido + Pastilla purificadora` (2 Carbón) en la cuadrícula →
  **Balde de agua limpia** (+60 sed). Detalle en `RECIPES.md`.
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

## Primera persona (`firstperson.rs`)

`V` alterna entre la **vista de águila** (por defecto: cámara ortográfica cenital
+ movimiento por clic) y una **primera persona tipo FPS**. En primera persona:

- La cámara pasa a **perspectiva** a la altura de los ojos del jugador; el ratón
  mira alrededor (cursor capturado, se libera solo al abrir un menú o el chat).
- **`WASD`** mueve relativo a hacia dónde miras, **`Espacio`** salta, **`Shift`**
  corre (misma física que la vista de águila — `player::step_player`).
- Minar / colocar / abrir cofres apunta al **centro de la pantalla** (mira), con
  una cruz. Las estaciones se usan con **`F`** (porque `W` es avanzar).
- Se ocultan el modelo del jugador, su sombra y el marcador de movimiento; el
  *cutout* de cámara y el punto-y-clic quedan en pausa (`eagle_view`).

Todo se revierte al pulsar `V` de nuevo (se restaura el zoom ortográfico).

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
| `W` | Usar estación cercana (banco / cofre / horno); selector si hay varias — en **primera persona** es `F` (W = avanzar) |
| `V` | Alternar **primera persona** (ratón para mirar, `WASD` para moverte) |
| `B` | Modo estancia (2 clics: esquinas → suelo + muros) |
| Barra espaciadora | Saltar |
| `G` | Comer / beber junto al agua / llenar botella vacía junto al agua |
| Clic izq. cerca de un animal | Golpearlo (o alimentarlo si llevas su comida) |
| Clic izq. con **Hoz** sobre tierra | Arar |
| Clic izq. con **Semillas** sobre tierra arada | Plantar trigo |
| Clic izq. sobre **Molino manual** | Abrir su panel (mantener **MOLER** para moler) |
| Shift | Correr (o paso grande al cambiar capa) |
| `Intro` | Abrir la línea de chat / enviar (`/` = comando; ver *Chat y comandos*) |
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

`src/` está agrupado por dominio en subcarpetas. Cada carpeta reexporta sus
módulos planos en la raíz del crate (`main.rs`), así que el código sigue usando
`crate::block::…`, `crate::camera::…`, etc. sin cambios.

### `src/world/` — el mundo voxel

| Archivo | Responsabilidad |
|---------|-----------------|
| `block.rs` | Tipos de voxel y sus propiedades (color, opacidad, colisión, agua/desgaste) |
| `chunk.rs` | Almacenamiento de chunk y helpers de coordenadas |
| `worldgen.rs` | Generación procedural (ruido → `ChunkData`): terreno, ríos, cuevas, agua contaminada, estructuras, ciudades en ruinas |
| `mesher.rs` | `ChunkData` → malla por descarte de caras (corte de capa, UVs de atlas) |
| `chunk_material.rs` | Material de chunk (`ExtendedMaterial` + shader de cutout) + material de agua |
| `block_atlas.rs` | Monta el atlas de texturas de bloque en runtime |
| `props.rs` | Modelos/props sobre bloques (banco, cofre, horno, antorcha, fogata, cultivos) |
| `streaming.rs` | Carga/descarga infinita, tareas en segundo plano, re-mallado, `set_block` |
| `structure.rs` | Formato `.json` de estructuras + `load_structure` / `stamp_structure` + librería |
| `scatter.rs` | Palos y plantas en el suelo (generación y recogida) |
| `view.rs` | Capa de visión (corte en Y) y sus controles |
| `daynight.rs` | Ciclo día/noche: sol, cielo y luz ambiental según `GameClock` |
| `animal.rs` | Manadas que deambulan, se golpean, se alimentan, se reproducen y sueltan recursos |

### `src/player/` — jugador y control

| Archivo | Responsabilidad |
|---------|-----------------|
| `mod.rs` | Spawn, movimiento (point-&-click + `step_player` compartido), colisión, modelo/held-item |
| `camera.rs` | Cámara ortográfica de seguimiento (vista de águila) |
| `firstperson.rs` | Vista en primera persona alternable (`V`): cámara perspectiva, mouse-look, WASD |
| `interact.rs` | Raycast a celda, minado temporizado, talar/colocar, modo estancia, durabilidad |
| `keybinds.rs` | Controles remapeables (`Action` → `KeyCode`) + pantalla de rebind |
| `skins.rs` | Catálogo de skins + pantalla para verlas y elegirlas |
| `station.rs` | Tecla `W` / `F`: escaneo + selector de estación (banco / cofre / horno / fogata) |

### `src/survival/` — supervivencia

| Archivo | Responsabilidad |
|---------|-----------------|
| `mod.rs` | Vida / hambre / sed / radiación / intoxicación, extremidades, comer-beber (`G`), muerte |
| `farming.rs` | Arado (hoz), hidratación de tierra arada, siembra y crecimiento del trigo |
| `fishing.rs` | Caña, lanzamiento sobre agua y pesca |
| `radiation.rs` | Granulado en pantalla + contador Geiger cerca de agua irradiada |

### `src/item/` — items e inventario

| Archivo | Responsabilidad |
|---------|-----------------|
| `mod.rs` | Items, inventario 5×10, hotbar, herramientas, crafteo (UI), desgaste (`Stack.wear`), paper doll |
| `container.rs` | Cofres, horno, molino y fogata: almacenamiento, fundido/hervido, UI, loot de ruinas |

### `src/ui/` — interfaz

| Archivo | Responsabilidad |
|---------|-----------------|
| `menu.rs` | Front-end: Jugar / Multijugador / Configuración (+ Gráficos: FPS, SSAO) / Salir |
| `pause.rs` | Menú de pausa (`Esc`), estado `GameFlow` y condición `not_paused` |
| `hud.rs` | Lectura de depuración en pantalla |
| `editor.rs` | Modo Structure Editor: estado propio, cámara libre, browser de contenido, exportar `.json` |

### `src/net/` — red

| Archivo | Responsabilidad |
|---------|-----------------|
| `mod.rs` | Multijugador TCP casero: modos Host/Client/Server, sync de ediciones + jugadores + chat |
| `command.rs` | Comandos de chat (`/biome`, `/structure`, `/city`, `/spawn`, …) |
| `discord.rs` | Discord Rich Presence en hilo aparte (estado del jugador) |

### `src/` (raíz)

| Archivo | Responsabilidad |
|---------|-----------------|
| `main.rs` | `App`, plugins, servidor dedicado, ícono de ventana; reexporta los módulos de cada carpeta |
| `save.rs` | Mundos (`saves/*.json`), guardado/carga, guardado del jugador en el servidor |

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
