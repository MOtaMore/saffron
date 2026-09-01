# Recetas — Aves

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
| Romper piedra | Pico | Piedra |
| **Pescar** | Caña de pescar | **Pescado** |
| **Arar** tierra/hierba (clic izq.) | **Hoz de pedernal** | **Tierra arada** |
| Plantar en tierra arada (clic izq.) | **Semillas** | crece **Trigo** (~3 min) |
| Romper trigo maduro | mano | **Trigo** ×1 (+ **Semillas** ×1, 50 %) |
| Moler (panel del molino: mantener **MOLER**) | **Trigo** + molino manual | **Harina** ×1 (gasta hambre) |
| Matar **Cerdo** | golpear (clic izq.) | **Carne** ×2 + **Grasa** ×1 |
| Matar **Vaca** | golpear | **Carne roja** ×3 + **Cuero** ×2 |
| Matar **Oveja** | golpear | **Cordero** ×2 + **Lana** ×2 |
| Matar **Pollo** | golpear | **Carne blanca** ×1 + **Pluma** ×2 |

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
Se coloca como el horno. Clic izquierdo **abre su panel**: ranura `[1]` para el
Trigo, botón **MOLER** (mantener pulsado → gasta hambre) y ranura `[2]` con la
Harina. Shift + mantener clic para romperlo (te devuelve el contenido).

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
| Grava | Piedra |
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
| Beber junto al agua (`G` sin comida en la mano) | — | +28 |
| Llenar **Botella vacía** (`G` junto al agua) | — | — (→ Botella de agua) |

El **hambre** sólo baja al hacer algo (caminar/correr, trabajar el molino), nunca
en reposo. La **sed** baja siempre (100 → 0 en 12:30 sin correr). A 0, cualquiera
quita vida (despacio). Con ambas > 60 % la vida se regenera. Beber/llenar vale
desde la orilla, vadeando o nadando.

---

## Ideas / pendiente (rellena libremente)

- [x] Madera a partir de Troncos (intermedio para banco / cofre)
- [x] Animales que sueltan comida y recursos al morir
- [x] Ciclo día/noche + vida/hambre/sed
- [x] Antorchas (Palo + Soga + Carbón vegetal) — dan luz de noche
- [x] Botellas de agua (2 Cristal) — sed portátil
- [x] Agricultura: hoz → tierra arada → trigo → molino → harina → masa → pan
- [x] Cebo/alimentación de animales con su cultivo favorito
- [ ] Herramientas de piedra (tras las de pedernal)
- [ ] Cuero → armadura / mochila · Lana → cama · Pluma → flechas mejores
- [ ] Cama, puerta, escalera…
