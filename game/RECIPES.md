# Recetas — Saffron

Referencia de todos los crafteos, fundidos y formas de conseguir materiales.
Documento vivo: edítalo para ir dándole forma al árbol de progresión.

> **Cómo funciona el crafteo:** la cuadrícula usa **formas**, como en Minecraft.
> Importa la disposición relativa de los objetos, no en qué parte de la
> cuadrícula la pongas: la forma puede ir arriba, abajo, a un lado… y su
> **reflejo horizontal** también vale. Cada casilla ocupada gasta **1 unidad**
> al craftear. Las recetas *sin forma* (marcadas abajo) solo miran qué objetos
> hay, en cualquier casilla.
>
> En los diagramas: cada `[ ]` es una casilla; `·` = casilla vacía.
>
> Fuente de datos: `src/item.rs` (`RECIPES`) y `src/container.rs` (fundido).

---

## 1. Recolección (sin crafteo)

| Acción | Necesita | Resultado |
|---|---|---|
| Romper **grava** | mano | **Pedernal** (~35 %) · si no, Grava |
| **Cortar plantas silvestres** (clic izq. cerca) | **Cuchillo de pedernal** | **Fibra vegetal** ×1 (+ **Semillas** ×1, ~40 %) |
| Pasar sobre **palos** del suelo | — | **Palo** |
| Pasar sobre **papas silvestres** del suelo | — | **Papa** |
| **Talar** un árbol | Hacha | **Tronco** (el tronco entero) |
| Romper tierra / hierba | Pala o Pico | Tierra |
| Romper **Roca** (`Stone`) | Pico | **Piedra** (`Cobblestone`) |
| Romper **Arcilla** | mano o Pala | **Bodoque de arcilla** ×4 |
| Romper **Barro** | mano o Pala | Barro |
| Romper un bloque de **Cemento** | Pico | **Ladrillo** (objeto) ×2 |
| Picar bloques de **Piedra / Roca Pulida / Ladrillos de Roca / Ladrillo / Cemento** | Pico | el mismo bloque (Cemento → Ladrillos) |
| **Pescar** | Caña de pescar | **Pescado** |
| **Arar** tierra/hierba (clic izq.) | **Hoz de pedernal** | **Tierra arada** |
| Plantar en tierra arada (clic izq.) | **Semillas** | crece **Trigo** (~3 min) |
| Romper trigo maduro | mano | **Trigo** ×1 (+ **Semillas** ×1, 50 %) |
| Moler (panel del molino: mantener **MOLER**) | **Trigo** + molino manual | **Harina** ×1 (gasta hambre) |
| Matar **Cerdo** | golpear (clic izq.) | **Carne** ×2 + **Grasa** ×1 |
| Matar **Vaca** | golpear | **Carne roja** ×3 + **Cuero** ×2 |
| Matar **Oveja** | golpear | **Cordero** ×2 + **Lana** ×2 |
| Matar **Pollo** | golpear | **Carne blanca** ×1 + **Pluma** ×2 |
| Abrir un **cofre de ciudad en ruinas** | — | loot: Pastillas purificadoras · **Botiquín** · Vodka · **Anti-Rad Meds** · Carbón (o vacío) |
| Recoger agua con un **Balde** metido en ella (`G`) | Balde | Balde de agua limpia / irradiada / tóxica según el tipo |

**Dónde aparecen los materiales nuevos** (generación del terreno, `worldgen.rs`):
**Arcilla** en parches en la arena de playas y en los bajíos de lagos y ríos;
**Barro** en las riberas de hierba pegadas a un río donde no llegó a haber arena.
Los bloques de mampostería (Piedra, Cemento, Roca Pulida, Ladrillos de Roca,
Ladrillo) forman las **ciudades en ruinas** que aparecen de vez en cuando en
tierra firme — rejilla claustrofóbica de bloques de pisos de hormigón medio
derrumbados, estilo soviético / *Samosbor*.

**Aparición del jugador:** nunca bajo el agua — al empezar un mundo el jugador
cae siempre sobre tierra firme (columna más cercana por encima del nivel del mar
y fuera de cauces de río).

---

## 2. Crafteo a mano — cuadrícula 2×2 (`I`)

### Madera ×4
```
[ Tronco ]
```
Un **Tronco** (lo que da talar) en cualquier casilla → 4 **Madera**. La Madera es
el material refinado que piden el banco y el cofre.

### Palos ×4
```
[ Tronco ]
[ Tronco ]
```

### Banco de trabajo ×1
```
[ Madera ][ Madera ]
[ Madera ][ Madera ]
```

### Botella vacía ×2
```
[ Cristal ]
[ Cristal ]
```
Dos vidrios, uno arriba del otro. Con `G` junto al agua se llena → **Botella de
agua** (portátil); al beberla recuperas sed y te queda la botella vacía.

### Soga de plantas ×1  *(sin forma)*
`3 × Fibra vegetal`, en cualquier casilla.

### Antorcha ×4  *(sin forma)*
`1 × Palo + 1 × Soga + 1 × Carbón vegetal`, en cualquier casilla. Da luz al
colocarse (útil de noche); se atraviesa y se rompe de un golpe.

### Masa ×1  *(sin forma)*
`1 × Harina + 1 × Botella de agua`. La botella **vuelve vacía**.
Se funde en el horno → **Pan**.

### Bloque de Arcilla ×1
```
[ Bodoque ][ Bodoque ]
[ Bodoque ][ Bodoque ]
```
4 **Bodoques de arcilla** se reagrupan en un bloque de **Arcilla** (el que pide el
Cemento).

### Ladrillo (bloque) ×4
```
[ Ladrillo ][ Ladrillo ]
[ Ladrillo ][ Ladrillo ]
```
4 **Ladrillos** (bodoques de arcilla fundidos en el horno) → 4 bloques de
**Ladrillo**.

### Cuchillo de pedernal ×1
```
[ Pedernal ]
[ Palo     ]
```

### Flecha de pedernal ×4
```
[ Pedernal ][ Palo ]
```

### Hacha de pedernal ×1  *(única herramienta que se hace a mano)*
```
[ Pedernal ][ Pedernal ]
[ Palo     ][ Pedernal ]
```

---

## 3. Crafteo en el banco de trabajo — cuadrícula 3×3

Pulsa **`W`** cerca de un Banco de trabajo colocado (radio 4) para abrirlo; si
hay varias estaciones cerca sale un selector.

### Pico de pedernal ×1
```
[ Pedernal ][ Pedernal ][ Pedernal ]
[    ·     ][ Palo     ][    ·     ]
[    ·     ][ Palo     ][    ·     ]
```

### Pala de pedernal ×1
```
[ Pedernal ]
[ Palo     ]
[ Palo     ]
```

### Caña de pescar rudimentaria ×1
```
[   ·  ][   ·  ][ Palo  ]
[   ·  ][ Palo ][   ·   ]
[ Soga ][ Fibra][   ·   ]
```

### Cofre ×1
```
[ Madera ][ Madera ][ Madera ]
[ Madera ][   ·    ][ Madera ]
[ Madera ][ Madera ][ Madera ]
```

### Horno ×1
```
[ Piedra ][ Piedra ][ Piedra ]
[ Piedra ][   ·    ][ Piedra ]
[ Piedra ][ Piedra ][ Piedra ]
```
8 × **Piedra** (`Cobblestone`, lo que da picar Roca).

### Hoz de pedernal ×1
```
[ Pedernal ][ Pedernal ]
[    ·     ][ Palo     ]
[    ·     ][ Palo     ]
```
Sólo sirve para arar tierra.

### Molino manual ×1
```
[ Piedra ][ Piedra ][ Piedra ]
[ Piedra ][ Palo   ][ Piedra ]
[ Piedra ][ Piedra ][ Piedra ]
```
8 × **Piedra** (`Cobblestone`) + Palo. Se coloca como el horno. Clic izquierdo
**abre su panel**: ranura `[1]` para el Trigo, botón **MOLER** (mantener pulsado →
gasta hambre) y ranura `[2]` con la Harina. Shift + mantener clic para romperlo
(te devuelve el contenido).

### Roca Pulida ×4
```
[ Roca ][ Roca ]
[ Roca ][ Roca ]
```
2×2 de **Roca** (`Stone`) → 4 **Roca Pulida**.

### Ladrillos de Roca ×4
```
[ Roca Pulida ][ Roca Pulida ]
[ Roca Pulida ][ Roca Pulida ]
```
2×2 de **Roca Pulida** → 4 **Ladrillos de Roca**.

### Cemento (mortero) ×4  *(sin forma)*
`2 × Arcilla (bloque) + 2 × Grava` → 4 **Cemento** (objeto, no bloque).
**Uso:** con el Cemento en la mano, **clic izquierdo sobre un bloque de Ladrillo**
lo fragua en un bloque de **Cemento** (gasta 1). Si ese bloque de Cemento se
rompe, sólo devuelve **Ladrillos (objeto) ×2** — el mortero se pierde.

### Pastilla purificadora ×2  *(sin forma)*
`2 × Carbón vegetal` → 2 **Pastillas purificadoras** (carbón activado).

### Vodka ×1  *(sin forma)*
`3 × Papa` (fermentadas). Beber (`G`) baja mucho la **intoxicación** (deshidrata
un poco).

---

## 3b. Agua contaminada — radiación e intoxicación

El mundo se está volviendo post-apocalíptico: parte de los **ríos** llevan **Agua
Irradiada** y algunas **lagunas / charcas interiores** llevan **Agua Tóxica**
(también hay charcos irradiados en las calles de las ciudades en ruinas).

- **Recoger:** con un **Balde** en la mano y `G` metido en el agua → *Balde de
  agua irradiada / tóxica* según el tipo (o *Balde de agua limpia* si es agua
  normal). Una botella de vidrio sólo se llena de agua **limpia**.
- **Beber crudo** (`G`) da sed pero sube **radiación** (+32) o **intoxicación**
  (+36); beber directo de la orilla, +16 / +20.
- **Purificar:**
  1. Pon el balde crudo en una **Fogata** (ranura 1) con combustible (ranura 2).
     Tras ~6 s → *Balde de agua hervida*.
  2. En la cuadrícula: `Balde de agua hervida + Pastilla purificadora` →
     **Balde de agua limpia** (`+60` sed, sin efectos). Recupera el balde vacío.
- **Curar el cuerpo:** radiación e intoxicación **bajan solas** (la radiación
  muy despacio). **Vodka** corta la intoxicación de golpe; **Anti-Rad Meds**
  (loot de ruinas) corta la radiación. Por encima de ~45 % cada una hace daño
  a la salud e impide regenerarla.

### Fogata ×1  *(a mano, 2×2)*
```
[ Palo ][ Palo ]
[ Roca ][ Roca ]
```
Se usa como el horno (`W`): ranura `[1]` balde crudo · `[2]` combustible ·
`[3]` balde hervido.

### Balde de madera ×1  *(a mano, 2×2)*
```
[ Madera ][ Madera ]
[ Madera ][        ]
```

---

## 4. Fundido — Horno

Ranuras: `[mineral]` `[combustible]` `[resultado]`. Funde en segundo plano,
**5 s por unidad** mientras haya combustible. La luz del modelo se enciende
mientras arde.

| Entrada | Salida |
|---|---|
| Pescado | Pescado cocinado |
| Carne (cerdo) | **Carne asada** |
| Carne roja (vaca) | **Carne roja asada** |
| Cordero (oveja) | **Cordero asado** |
| Carne blanca (pollo) | **Carne blanca asada** |
| **Masa** | **Pan** |
| Arena | Cristal |
| Grava | **Roca** (`Stone`) |
| **Piedra** (`Cobblestone`) | **Roca** (`Stone`) |
| **Bodoque de arcilla** | **Ladrillo** |
| Tronco | Carbón vegetal |

### Combustibles

| Combustible | Dura |
|---|---|
| Carbón vegetal | 14 s |
| Tronco | 16 s |
| **Grasa** | 10 s |
| Palo | 4 s |
| Hojas | 2 s |

---

## 5. Contenedores

| Cofre | 27 ranuras. Dos cofres pegados = **cofre doble** (54). |
|---|---|
| Romper un cofre / horno | Shift + mantener clic izquierdo (te devuelve el contenido) |

---

## 6. Comida y bebida (`G`)

| Item | Hambre | Sed |
|---|---|---|
| Carne / Carne roja / Cordero / Carne blanca (cruda) | +12 | — |
| **Carne asada / Carne roja asada / Cordero asado / Carne blanca asada** | +30 | — |
| **Pan** | +35 | — |
| **Papa** (cruda) | +10 | — |
| Pescado | +8 | +2 |
| Pescado cocinado | +20 | +2 |
| **Botella de agua** (`G`) — devuelve la botella vacía | — | +45 |
| Beber junto al agua **limpia** (`G` sin nada en la mano) | — | +28 |
| Llenar **Botella vacía** (`G` junto al agua limpia) | — | — (→ Botella de agua) |
| **Balde de agua limpia** — devuelve el balde | — | +60 |
| **Balde de agua hervida** — devuelve el balde | — | +45 (·+7 intoxicación residual) |
| **Balde de agua irradiada** (cruda) — devuelve el balde | — | +38 (·+32 radiación) |
| **Balde de agua tóxica** (cruda) — devuelve el balde | — | +38 (·+36 intoxicación) |
| **Vodka** | — | −6 (·−45 intoxicación) |
| **Anti-Rad Meds** (loot) | — | — (·−50 radiación) |
| **Botiquín** (loot de ruinas) | — | — (·+55 salud y a cada extremidad) |

El **hambre** sólo baja al hacer algo (caminar/correr, trabajar el molino), nunca
en reposo. La **sed** baja siempre (100 → 0 en 12:30 sin correr). A 0, hambre o sed
quita vida despacio; lo mismo la **radiación > 45 %** y la **intoxicación > 40 %**
(el daño escala con el nivel y bloquean la regeneración). Con hambre y sed > 60 %
y sin envenenamiento, la vida se regenera. Beber/llenar vale desde la orilla,
vadeando o nadando — pero nadar en agua contaminada también te contamina.

## 7. Desgaste

- **Durabilidad**: cada herramienta (cuchillo, hacha, pico, pala, hoz, caña) se
  gasta al usarse (romper bloques, talar, arar, pescar, cazar). El tooltip
  muestra `(XX% dur.)`; al llegar a 0 se rompe y desaparece. Aguante base: pico
  250 usos · pala 220 · hacha 200 · cuchillo 150 · hoz 140 · caña 80.
- **Podredumbre**: la comida se pudre con el tiempo (tooltip `(XX% fresh)` →
  `(going off!)`). La **cruda** (carne, pescado, papa) dura ~6 min; la
  **cocinada** (asados, pescado cocinado, pan) ~16 min. Rancia alimenta menos y,
  casi podrida, intoxica; del todo podrida se vuelve **Rotten Food** (comerla
  intoxica bastante). *Los cofres no la conservan — pendiente.*
- **Oxidación**: infraestructura lista por debajo del capó (`WearKind::Rust`)
  para cuando haya armas de fuego / equipo metálico. Ningún objeto la usa aún.

---

## Ideas / pendiente (rellena libremente)

- [x] Madera a partir de Troncos (intermedio para banco / cofre)
- [x] Animales que sueltan comida y recursos al morir
- [x] Ciclo día/noche + vida/hambre/sed
- [x] Antorchas (Palo + Soga + Carbón vegetal) — dan luz de noche
- [x] Botellas de agua (2 Cristal) — sed portátil
- [x] Agricultura: hoz → tierra arada → trigo → molino → harina → masa → pan
- [x] Cebo/alimentación de animales con su cultivo favorito
- [x] Cadena de mampostería: Roca → Piedra → Roca Pulida → Ladrillos de Roca;
      Arcilla → Bodoque → Ladrillo → bloque de Ladrillo; Cemento (Arcilla + Grava)
- [x] Radiación / intoxicación + Agua Irradiada/Tóxica + Fogata/Balde/Pastillas +
      Vodka / Anti-Rad + loot de cofres de ruinas (giro post-apocalíptico soviético)
- [ ] Herramientas de piedra (tras las de pedernal)
- [ ] Contador Geiger · traje anti-radiación · máscara de gas
- [ ] Más loot y tablas por tipo de cofre; sincronizar cofres en multijugador
- [ ] Cuero → armadura / mochila · Lana → cama · Pluma → flechas mejores
- [ ] Cama, puerta, escalera…
