# BACKTESTER DE ALTA VELOCIDAD — Rust + Tauri 2 + React

## Identidad del Proyecto

Aplicación de escritorio profesional para backtesting de estrategias de trading. El usuario sube datos históricos (CSV), construye estrategias visualmente (no-code), ejecuta backtests y optimiza parámetros. Debe ser ultrarrápida: capaz de procesar años de data tick (~20GB) en segundos gracias a Rust.

---

## Stack Tecnológico

### Backend (Rust — src-tauri/)

| Propósito | Crate | Notas |
|---|---|---|
| Framework desktop | `tauri` 2.x | Usar la última versión estable de Tauri 2. Features: dialog, fs, shell |
| Procesamiento de datos | `polars` (última estable) | Siempre usar modo LazyFrame. Features: lazy, parquet, temporal, dtype-datetime |
| Indicadores técnicos | Implementación manual | NO usar la crate `ta`. Implementar cada indicador manualmente con Polars para máximo control y rendimiento. Ver sección de indicadores abajo |
| Paralelización | `rayon` | Para optimización multi-core |
| Base de datos | `rusqlite` con feature "bundled" | Para metadata de símbolos, estrategias guardadas y resultados |
| Serialización | `serde` + `serde_json` | Con feature "derive" |
| Async runtime | `tokio` con features "full" | |
| Fechas | `chrono` con feature "serde" | |
| IDs | `uuid` con features "v4", "serde" | |
| Errores | `thiserror` + `anyhow` | thiserror para errores tipados del dominio, anyhow para propagación |
| Aleatoriedad | `rand` | Para algoritmo genético |
| Logging | `tracing` + `tracing-subscriber` | Logging estructurado en todo el backend |
| CSV | `csv` | Para importación y exportación |
| Matemáticas | `statrs` | Para distribuciones y cálculos estadísticos |

### Frontend (React + TypeScript — src/)

| Propósito | Paquete | Notas |
|---|---|---|
| UI Framework | React 18 + TypeScript 5 | |
| Bundler | Vite 5 | |
| Tauri API | `@tauri-apps/api` v2 | IMPORTANTE: usar la API v2, no v1 |
| Gráficos | `recharts` | Para equity curve, drawdown, histogramas |
| Componentes | `@radix-ui/*` | Primitivos accesibles: select, dialog, tabs, tooltip, dropdown-menu |
| Estilos | Tailwind CSS 3 | Con clsx + tailwind-merge para composición |
| Estado global | `zustand` | Un solo store principal para toda la app |
| Estado async | `@tanstack/react-query` v5 | Para queries a comandos Tauri |
| Formularios | `react-hook-form` + `zod` | Validación del lado del frontend |
| Iconos | `lucide-react` | |
| Fechas | `date-fns` | |
| Utilidades CSS | `class-variance-authority` | Para variantes de componentes |

### Formatos de Datos

- **Input del usuario:** CSV (tick data o barras OHLCV)
- **Storage interno:** Parquet con compresión Snappy (conversión automática al importar)
- **Metadata:** SQLite (info de símbolos, estrategias guardadas, historial de backtests)
- **Estrategias:** JSON (guardadas en SQLite y opcionalmente como archivos)

---

## Arquitectura del Proyecto

```
backtester/
├── CLAUDE.md                         # ← Este archivo
├── src-tauri/                        # Backend Rust
│   ├── src/
│   │   ├── main.rs                   # Entry point, setup Tauri, init DB
│   │   ├── commands.rs               # Todos los comandos Tauri
│   │   ├── errors.rs                 # Enum de errores con thiserror
│   │   │
│   │   ├── data/                     # Gestión de datos
│   │   │   ├── mod.rs
│   │   │   ├── validator.rs          # Validación y detección de formato CSV
│   │   │   ├── loader.rs             # CSV → Parquet, carga de Parquet
│   │   │   ├── converter.rs          # Conversión de timeframes
│   │   │   └── storage.rs            # Operaciones SQLite
│   │   │
│   │   ├── engine/                   # Motor de backtesting
│   │   │   ├── mod.rs
│   │   │   ├── indicators.rs         # Cálculo de todos los indicadores
│   │   │   ├── strategy.rs           # Evaluación de reglas y estrategias
│   │   │   ├── executor.rs           # Loop principal del backtest
│   │   │   ├── position.rs           # Gestión de posiciones abiertas
│   │   │   ├── orders.rs             # Sistema de órdenes
│   │   │   ├── metrics.rs            # Cálculo de métricas de rendimiento
│   │   │   └── optimizer.rs          # Grid Search + Algoritmo Genético
│   │   │
│   │   ├── models/                   # Todas las estructuras de datos
│   │   │   ├── mod.rs
│   │   │   ├── candle.rs             # Estructura OHLCV
│   │   │   ├── tick.rs               # Estructura tick
│   │   │   ├── trade.rs              # Trade ejecutado con todos sus campos
│   │   │   ├── strategy.rs           # Definición de estrategia completa
│   │   │   ├── rule.rs               # Reglas, operandos, comparadores
│   │   │   ├── config.rs             # Configuración de backtest y costos
│   │   │   └── result.rs             # Resultados y métricas
│   │   │
│   │   └── utils/
│   │       ├── mod.rs
│   │       ├── math.rs               # Funciones matemáticas reutilizables
│   │       └── export.rs             # Exportación CSV/PDF
│   │
│   ├── Cargo.toml
│   └── tauri.conf.json
│
├── src/                              # Frontend React
│   ├── components/
│   │   ├── layout/                   # AppLayout, Sidebar, Header
│   │   ├── data/                     # FileUploader, DataList, DataPreview
│   │   ├── strategy/                 # StrategyBuilder, RuleBuilder, ConfigPanel
│   │   ├── backtest/                 # BacktestPanel, MetricsGrid, EquityCurve, etc.
│   │   ├── optimization/            # OptimizerPanel, ParameterRanges, HeatmapChart
│   │   └── ui/                       # Componentes reutilizables (Button, Input, Select, etc.)
│   │
│   ├── stores/
│   │   └── useAppStore.ts            # Store Zustand principal
│   │
│   ├── hooks/                        # Custom hooks para Tauri commands
│   ├── lib/
│   │   ├── types.ts                  # Tipos TypeScript (mirror de los modelos Rust)
│   │   ├── tauri.ts                  # Wrappers tipados de invoke()
│   │   └── utils.ts
│   │
│   ├── App.tsx
│   ├── main.tsx
│   └── index.css
│
├── data/                             # Datos del usuario (gitignored)
│   ├── symbols/                      # Archivos Parquet por símbolo
│   ├── strategies/                   # JSONs de estrategias
│   └── backtester.db                 # SQLite
│
├── package.json
├── tsconfig.json
├── tailwind.config.js
└── vite.config.ts
```

---

## Especificaciones por Módulo

### 1. GESTIÓN DE DATOS

#### Formatos CSV aceptados:

**Tick data:**
```
DateTime,Bid,Ask,Volume
2024-01-01 00:00:00.123,1.0850,1.0851,100
```

**Barras OHLCV (cualquier timeframe):**
```
Date,Time,Open,High,Low,Close,Volume
2024-01-01,00:00,1.0850,1.0855,1.0848,1.0852,1000
```

El validador debe auto-detectar si es tick o barra analizando las columnas.

#### Flujo de importación:
1. Usuario selecciona CSV via dialog nativo de Tauri o drag & drop
2. Validar formato y detectar tipo (tick vs barra)
3. Convertir a Parquet con Polars (mostrar progreso via Tauri Events)
4. Si es tick o m1, generar automáticamente timeframes superiores: m1 → m5 → m15 → m30 → h1 → h4 → d1
5. Guardar metadata en SQLite (nombre del símbolo, fechas, cantidad de filas, paths de archivos)

#### Conversión de timeframes:
Usar Polars `group_by_dynamic` para agregar barras: first(open), max(high), min(low), last(close), sum(volume).

#### Base de datos SQLite — 3 tablas:
- **symbols**: id, name, base_timeframe, upload_date, total_rows, start_date, end_date, paths a cada timeframe Parquet
- **strategies**: id, name, created_at, updated_at, strategy_json (JSON completo)
- **backtest_results**: id, strategy_id (FK), symbol_id (FK), timeframe, executed_at, metrics_json, trades_count

---

### 2. MOTOR DE BACKTESTING

#### 2.1 Indicadores técnicos (13 en v1):

Todos implementados manualmente usando operaciones vectorizadas de Polars o iteradores de Rust. NO usar crates externas de indicadores.

| Indicador | Parámetros |
|---|---|
| SMA (Simple Moving Average) | period |
| EMA (Exponential Moving Average) | period |
| RSI (Relative Strength Index) | period |
| MACD | fast_period, slow_period, signal_period |
| Bollinger Bands | period, std_dev (retorna: upper, middle, lower) |
| ATR (Average True Range) | period |
| Stochastic | k_period, d_period (retorna: %K, %D) |
| ADX (Average Directional Index) | period |
| CCI (Commodity Channel Index) | period |
| ROC (Rate of Change) | period |
| Williams %R | period |
| Parabolic SAR | acceleration_factor, maximum_factor |
| VWAP (Volume Weighted Avg Price) | (sin parámetros, se resetea por sesión) |

Cada indicador debe tener un test unitario que verifique su cálculo contra valores conocidos.

#### 2.2 Sistema de reglas (no-code):

Una estrategia se compone de:
- **Entry rules**: Lista de reglas que deben cumplirse para abrir posición
- **Exit rules**: Lista de reglas que cierran una posición abierta
- Cada regla tiene: operando_izquierdo, comparador, operando_derecho, operador_lógico (AND/OR con la siguiente regla)

**Operandos posibles:**
- Indicador (cualquiera de los 13, con sus parámetros)
- Precio (Open, High, Low, Close)
- Constante numérica
- Valor del indicador N barras atrás (offset)

**Comparadores:**
- Mayor que (>), Menor que (<), Mayor o igual (>=), Menor o igual (<=), Igual (==)
- **CrossAbove**: el valor izquierdo cruza por encima del derecho (estaba debajo en la barra anterior, ahora está arriba)
- **CrossBelow**: el valor izquierdo cruza por debajo del derecho

**Operadores lógicos entre reglas:** AND, OR

#### 2.3 Configuración de instrumento (IMPORTANTE — configurable, no hardcoded):

Cada símbolo debe tener su configuración específica:

```
InstrumentConfig:
  - pip_size: f64          # 0.0001 para EUR/USD, 0.01 para USD/JPY, 1.0 para índices, etc.
  - pip_value: f64         # Valor monetario de 1 pip por 1 lote estándar
  - lot_size: f64          # Tamaño de 1 lote estándar (100,000 para Forex, 1 para crypto, etc.)
  - min_lot: f64           # Lote mínimo (ej: 0.01)
  - tick_size: f64         # Mínimo movimiento de precio
  - digits: usize          # Cantidad de decimales (5 para Forex, 2 para JPY pairs, etc.)
```

Esta configuración se pide al importar un nuevo símbolo, con presets para los tipos más comunes (Forex major, Forex JPY, Crypto, Índices).

#### 2.4 Position Sizing:
- **Lotes fijos**: Siempre el mismo tamaño (ej: 1.0 lote)
- **Monto fijo**: Un monto en dinero por trade (ej: $1,000)
- **Porcentaje del equity**: Un % del capital actual (ej: 2%)
- **Risk-based**: Calcular lotes basándose en distancia al stop loss y % de riesgo máximo por trade

#### 2.5 Stop Loss:
- En pips (usando pip_size del instrumento, no hardcoded)
- En porcentaje del precio de entrada
- Basado en ATR (multiplicador × ATR actual)

#### 2.6 Take Profit:
- En pips (opcional)
- En ratio risk-reward (ej: 2:1 respecto al SL)
- Basado en ATR (multiplicador × ATR actual)

#### 2.7 Trailing Stop:
- Basado en ATR (se mueve con cada nueva barra si el precio avanza a favor)
- Basado en ratio risk-reward

#### 2.8 Tipos de órdenes:
- Market (ejecución inmediata al precio actual)
- Limit (ejecución cuando el precio toca el nivel especificado)
- Stop (ejecución cuando el precio rompe el nivel especificado)
- Stop-Limit (combinación)

#### 2.9 Costos de trading (configurables por símbolo):
- **Spread**: En pips. Se aplica al abrir el trade
- **Commission**: En % del valor de la posición o monto fijo por lote
- **Slippage**: En pips. Aleatorio o fijo, emula deslizamiento de precio real

#### 2.10 Configuración general del backtest:
- Capital inicial
- Leverage
- Símbolo a testear
- Timeframe
- **Rango de fechas** (fecha inicio y fecha fin — filtrar data antes de ejecutar)
- Dirección permitida: Solo Long, Solo Short, o Ambas

#### 2.11 Ejecución del backtest:

El executor itera barra por barra sobre los datos y en cada barra:
1. Actualizar posiciones abiertas (verificar si se tocó SL, TP, trailing stop)
2. Evaluar exit rules para posiciones abiertas
3. Evaluar entry rules si no hay posición (o si se permiten múltiples posiciones)
4. Registrar trade si se abre o cierra posición
5. Actualizar equity curve

Solo se permite **una posición abierta a la vez** (en v1). Si hay posición abierta, no se evalúan entry rules.

#### 2.12 Métricas de rendimiento:

El cálculo de métricas debe incluir TODAS las siguientes:

**Retornos:**
- Capital final, retorno total (%), retorno anualizado (%), retorno mensual promedio (%)

**Risk-adjusted:**
- Sharpe Ratio (anualizado, √252), Sortino Ratio (solo downside deviation), Calmar Ratio (return/maxDD)

**Drawdown:**
- Max drawdown (%), duración del max drawdown (en barras y en tiempo), drawdown promedio, Recovery Factor (net profit / max DD)

**Trades:**
- Total trades, winning, losing, break-even, win rate (%)

**P&L:**
- Gross profit, gross loss, net profit, Profit Factor (gross profit / gross loss), avg trade, avg win, avg loss, largest win, largest loss, Expectancy

**Consistencia:**
- Max consecutive wins, max consecutive losses, avg consecutive wins, avg consecutive losses

**Tiempo:**
- Avg trade duration, avg bars in trade, avg duración de winners vs losers

**Risk:**
- MAE (Maximum Adverse Excursion) promedio y máximo
- MFE (Maximum Favorable Excursion) promedio y máximo

#### 2.13 Datos de salida del backtest:
- Lista completa de trades (con todos sus campos: entrada, salida, P&L, razón de cierre, duración, MAE, MFE)
- Equity curve (array de puntos timestamp + equity)
- Drawdown curve (array de puntos timestamp + drawdown %)
- Array de retornos por trade (para histograma)
- Struct de métricas completa

---

### 3. OPTIMIZACIÓN

#### 3.1 Grid Search:
- El usuario selecciona qué parámetros optimizar (ej: "SMA period de la regla 1")
- Define min, max y step para cada parámetro
- Se generan todas las combinaciones (producto cartesiano)
- Cada combinación ejecuta un backtest completo
- **Paralelizar con rayon** (usar todos los cores disponibles)
- Mostrar progreso via Tauri Events (combinación actual / total, ETA estimado)
- Límite de seguridad: máximo 50,000 combinaciones (advertir al usuario si supera)

#### 3.2 Algoritmo Genético:
- Parámetros del GA: population_size, generations, mutation_rate, crossover_rate
- Tournament selection (torneo de 3)
- Crossover de un punto
- Mutación aleatoria dentro de los rangos definidos
- Elitismo: mantener el mejor individuo de cada generación
- **Paralelizar evaluación de fitness con rayon**
- Reportar progreso por generación

#### 3.3 Función objetivo (qué optimizar):
- Total Profit
- Sharpe Ratio
- Profit Factor
- Win Rate
- Custom (futuro)

#### 3.4 Resultados de optimización:
- Lista ordenada de los mejores resultados (parámetros + métricas)
- Heatmap 2D cuando hay exactamente 2 parámetros (eje X = param1, eje Y = param2, color = objetivo)

---

### 4. FRONTEND

#### 4.1 Layout:
- Sidebar izquierda con navegación: Datos, Estrategia, Backtest, Optimización
- Área principal que cambia según la sección seleccionada
- Header con nombre de la app y símbolo/estrategia activa
- Dark mode por defecto (toggle para light mode)

#### 4.2 Sección Datos:
- Botón para importar CSV (abre dialog nativo de Tauri)
- Drag & drop como alternativa
- Al importar: formulario para nombrar el símbolo y configurar el instrumento (pip_size, lot_size, etc.) con presets
- Progress bar durante conversión CSV → Parquet
- Lista de símbolos importados con info (nombre, fechas, rows, timeframes disponibles)
- Preview de las primeras filas de datos
- Botón para eliminar símbolo

#### 4.3 Sección Estrategia (Strategy Builder):
- Constructor visual de reglas tipo: [Indicador/Precio/Valor] [Comparador] [Indicador/Precio/Valor]
- Secciones separadas para Entry Rules y Exit Rules
- Cada regla es una fila con dropdowns y inputs numéricos
- Botón para agregar/eliminar reglas
- Conector lógico AND/OR entre reglas
- Panel de configuración: position sizing, stop loss, take profit, trailing stop, costos
- Guardar/cargar estrategias (nombre + JSON)
- Lista de estrategias guardadas

#### 4.4 Sección Backtest:
- Selector de símbolo y timeframe
- Selector de rango de fechas (date pickers)
- Botón "Run Backtest" (deshabilitado si falta config)
- Mientras corre: spinner o progress indicator
- Resultados:
  - **MetricsGrid**: Grid con todas las métricas clave en cards/badges (similar a un dashboard)
  - **EquityCurve**: Gráfico de línea (Recharts) mostrando evolución del capital
  - **DrawdownChart**: Gráfico de área mostrando drawdown % (en rojo/negativo)
  - **ReturnsHistogram**: Histograma de distribución de retornos por trade
  - **TradesList**: Tabla paginada con todos los trades (sorteable por columnas)
- Botón para exportar trades a CSV
- Botón para exportar reporte completo (resumen + métricas + gráficos) — v1 puede ser CSV, PDF en futuro

#### 4.5 Sección Optimización:
- Selector de método (Grid Search / Genetic Algorithm)
- Para cada parámetro optimizable: nombre descriptivo del parámetro, min, max, step
- Selector de función objetivo
- Config específica de GA (population, generations, rates) si se elige GA
- Botón "Run Optimization" con progress bar (% completado, ETA)
- **Botón de cancelación** que detiene la optimización
- Resultados:
  - Tabla con top 10-20 resultados (parámetros, objetivo, retorno, sharpe, max DD, trades)
  - Heatmap 2D cuando aplica
  - Botón para aplicar parámetros del mejor resultado a la estrategia actual

#### 4.6 Store global (Zustand):
El store debe manejar:
- Lista de símbolos importados y símbolo seleccionado
- Estrategia actual (con todas sus reglas y config)
- Lista de estrategias guardadas
- Resultados del último backtest
- Estado de operaciones en curso (isLoading, progress %)
- Configuración de la app (dark mode, etc.)

---

### 5. COMUNICACIÓN FRONTEND ↔ BACKEND

#### Comandos Tauri (cada uno es un #[tauri::command]):
- `upload_csv(file_path, symbol_name, instrument_config)` → symbol_id
- `get_symbols()` → Vec<Symbol>
- `delete_symbol(symbol_id)` → ()
- `preview_data(symbol_id, timeframe, limit)` → Vec<Row>
- `run_backtest(strategy)` → BacktestResults
- `cancel_backtest()` → ()
- `run_optimization(strategy, optimization_config)` → Vec<OptimizationResult>
- `cancel_optimization()` → ()
- `save_strategy(strategy)` → strategy_id
- `load_strategies()` → Vec<Strategy>
- `delete_strategy(strategy_id)` → ()
- `export_trades_csv(trades, file_path)` → ()

#### Tauri Events (backend → frontend, para progreso):
- `conversion-progress` → { percent: u8, message: String }
- `backtest-progress` → { percent: u8, current_bar: usize, total_bars: usize }
- `optimization-progress` → { percent: u8, current: usize, total: usize, best_so_far: f64, eta_seconds: u64 }

#### Cancelación de operaciones largas:
Usar un `AtomicBool` compartido. El comando `cancel_*` lo pone en `true`. El loop del backtest/optimización lo verifica en cada iteración y aborta si está en `true`.

---

## Reglas de Desarrollo

### Rust:
1. **Compilar después de cada archivo nuevo** — ejecutar `cargo check` y no avanzar si hay errores
2. **Nunca usar `.unwrap()` en código de producción** — solo en tests. Usar `?` con Result o `thiserror` para errores
3. **Definir TODOS los errores en `errors.rs`** con un enum tipado usando `thiserror`
4. **Usar `tokio::sync::Mutex`** (no `std::sync::Mutex`) para el estado compartido en comandos async de Tauri
5. **Polars siempre en modo LazyFrame** — solo llamar `.collect()` al final de la cadena
6. **Documentar funciones públicas** con `///` doc comments
7. **Tests unitarios** para: cada indicador, evaluación de reglas, cálculo de métricas, position sizing, stop loss
8. **Logging con `tracing`** — info para operaciones importantes, debug para detalles, error para fallos

### React/TypeScript:
1. **Tipos estrictos** — nunca usar `any`. Definir interfaces para todo
2. **Los tipos TypeScript deben ser mirror exacto de los structs Rust** — mantenerlos sincronizados
3. **Componentes funcionales** con hooks, nunca class components
4. **Validación con Zod** en todos los formularios antes de enviar a Rust
5. **Manejar estados de error y loading** en toda interacción con el backend
6. **Debouncing** en inputs numéricos para no bombardear el backend

### General:
1. **Una fase a la vez** — no avanzar a la siguiente hasta que la actual compile, tenga tests pasando, y la UI funcione
2. **Git commit al final de cada fase**
3. **Errores claros al usuario** — nunca mostrar errores técnicos de Rust en la UI sin formatear

---

## Fases de Implementación

### Fase 0 — Setup (base del proyecto)
1. Crear proyecto Tauri 2 con template React + TypeScript
2. Instalar todas las dependencias de Rust y Node
3. Configurar Tailwind CSS
4. Crear estructura de carpetas completa
5. Crear el enum de errores en `errors.rs`
6. Inicializar SQLite con las 3 tablas
7. Crear componentes UI base (Button, Input, Select, Card, Dialog, Tabs, Table)
8. Crear el layout principal (Sidebar + área de contenido)
9. Crear el store Zustand vacío con la estructura definida
10. Verificar que `cargo tauri dev` funciona y muestra la UI

### Fase 1 — Datos (importación y conversión)
1. `data/validator.rs` — Detectar y validar CSV (tick vs barra)
2. `data/loader.rs` — Convertir CSV a Parquet con Polars
3. `data/converter.rs` — Generar timeframes superiores
4. `data/storage.rs` — CRUD de SQLite para símbolos
5. Comandos: `upload_csv`, `get_symbols`, `delete_symbol`, `preview_data`
6. Tauri Event: `conversion-progress`
7. Frontend: FileUploader (con dialog nativo), DataList, InstrumentConfigForm, DataPreview, ConversionProgress
8. **Test**: Importar un CSV real, verificar que genera Parquet y todos los timeframes

### Fase 2 — Engine Core (indicadores + reglas + ejecución)
1. `models/` — Definir TODAS las structs (candle, tick, trade, strategy, rule, config, result)
2. `engine/indicators.rs` — Implementar los 13 indicadores con tests unitarios
3. `engine/strategy.rs` — Evaluación de reglas (incluyendo CrossAbove/CrossBelow)
4. `engine/position.rs` — Gestión de posición abierta
5. `engine/orders.rs` — Procesamiento de órdenes y costos
6. `engine/executor.rs` — Loop principal del backtest barra por barra
7. `engine/metrics.rs` — Cálculo de TODAS las métricas definidas
8. Comando: `run_backtest` con Tauri Event `backtest-progress`
9. **Test**: Crear una estrategia simple (SMA cross), ejecutar backtest, verificar que los trades y métricas sean coherentes

### Fase 3 — Strategy Builder (frontend)
1. RuleBuilder — Constructor visual de reglas con dropdowns
2. IndicatorSelector — Dropdown con los 13 indicadores y sus parámetros
3. ComparatorSelector — Dropdown de comparadores
4. ConfigPanel — Position sizing, SL, TP, trailing stop, costos
5. StrategyList — Guardar/cargar/eliminar estrategias
6. Comandos: `save_strategy`, `load_strategies`, `delete_strategy`
7. Conectar todo con el store Zustand
8. **Test**: Crear una estrategia en la UI, guardarla, cargarla, verificar que el JSON sea correcto

### Fase 4 — Visualización de Resultados
1. BacktestPanel — Botón run + selección de símbolo/timeframe/fechas
2. MetricsGrid — Dashboard de métricas en cards
3. EquityCurve — Gráfico de línea con Recharts
4. DrawdownChart — Gráfico de área
5. ReturnsHistogram — Histograma de distribución
6. TradesList — Tabla paginada y sorteable
7. Integrar todo: crear estrategia → correr backtest → ver resultados
8. **Test end-to-end**: Flujo completo desde la UI

### Fase 5 — Optimización
1. `engine/optimizer.rs` — Grid Search con rayon
2. `engine/optimizer.rs` — Algoritmo Genético con rayon
3. Cancelación con AtomicBool
4. Comandos: `run_optimization`, `cancel_optimization`
5. Tauri Event: `optimization-progress`
6. Frontend: OptimizerPanel, ParameterRanges, MethodSelector, OptimizationProgress, ResultsTable
7. HeatmapChart (cuando hay 2 parámetros)
8. Botón "Apply best parameters" que actualiza la estrategia
9. **Test**: Optimizar un parámetro simple, verificar que encuentra mejores valores

### Fase 6 — Exportación y Polish
1. Exportar trades a CSV
2. Exportar reporte de métricas
3. Dark mode toggle
4. Loading states consistentes en toda la app
5. Error handling en UI (toasts o alerts para errores)
6. Tooltips explicativos en métricas y configuraciones
7. Keyboard shortcuts (Ctrl+S guardar estrategia, Ctrl+Enter correr backtest)
8. Validación robusta de inputs (no permitir valores negativos, rangos lógicos, etc.)

---

## Optimizaciones de Performance

### Compilación Release:
```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
panic = 'abort'
strip = true
```

### Runtime:
- Polars LazyFrame siempre (no evaluar hasta necesitar los datos)
- Cache de indicadores ya calculados (HashMap<indicador+params, Vec<f64>>) para no recalcular en optimización
- Rayon para paralelizar optimización (nunca el backtest individual)
- Evitar `.clone()` innecesarios — usar referencias siempre que sea posible
- Cargar datos Parquet una sola vez y reutilizar para múltiples backtests en optimización

---

## Roadmap Post-v1 (NO implementar ahora)

Estas features están planeadas pero NO son parte de v1. Documentarlas aquí como referencia futura:

1. Chart de velas con señales de entrada/salida (usando lightweight-charts)
2. Walk-forward Analysis (train/test split automático)
3. Monte Carlo Simulation
4. Portfolio Backtesting (múltiples símbolos simultáneos)
5. Filtro de sesiones de mercado (ej: solo sesión de Londres)
6. Margin call / liquidación
7. Machine Learning integration
8. Live trading (conexión a brokers via API)
9. Multi-timeframe strategies (ej: señal en H4, entrada en M15)

---

## Análisis del Motor — Bugs Corregidos y Pendientes (2026-03)

### Bugs corregidos — sesión anterior
| Archivo | Línea | Severidad | Fix aplicado |
|---|---|---|---|
| `engine/executor.rs` | 341 | CRÍTICA | `equity += trade.pnl - trade.commission` en cierre end-of-data |
| `engine/metrics.rs` | 346 | CRÍTICA | Sortino: denominador cambiado de `neg_count` a `n` (total trades) |
| `engine/metrics.rs` | 93-96 | CRÍTICA | Recovery Factor: usa drawdown absoluto real de la equity curve, no `initial_capital × pct` |
| `engine/metrics.rs` | 36 | ALTA | Breakeven: `t.pnl == 0.0` → `t.pnl.abs() < 1e-6` |
| `engine/metrics.rs` | 102 | ALTA | Factor anualización: `total_trades * bars_per_year / trading_bars` (adaptativo) |
| `commands.rs` | 548 | ALTA | OOS: parámetro faltante → `warn!` + `continue` en vez de usar `0.0` silenciosamente |
| `commands.rs` | 541 | MEDIA | Cancelación respetada entre cada iteración del loop OOS |
| `engine/position.rs` | 84 | MEDIA | Lot sizing: `warn!` cuando `raw <= 0.0 \|\| NaN` |
| `engine/position.rs` | 104-107 | MEDIA | SL: `warn!` cuando SL queda por encima de entry (Long) o por debajo (Short) |
| `engine/optimizer.rs` | 760 | BAJA | Composite score: `range > 0.0` → `range > f64::EPSILON` para evitar Inf/NaN |

### Bugs corregidos — sesión 2026-03 (análisis completo)
| Archivo | Severidad | Fix aplicado |
|---|---|---|
| `engine/executor.rs` | CRÍTICA | `filter_dataframe_by_date`: end_date "YYYY-MM-DD" normalizado a "YYYY-MM-DD 23:59:59.999999" |
| `engine/executor.rs` | ALTA | ATR multi-period: struct `AtrValues {for_sl, for_tp, for_ts}` — cada componente usa su propio ATR period |
| `engine/position.rs` + `executor.rs` | ALTA | `CloseReason::TrailingStop` nunca emitido — flag `trailing_stop_activated` en `OpenPosition` |
| `commands.rs` | ALTA | `delete_symbol`: usa `remove_dir_all` para directorios de tick, `remove_file` para archivos |
| `lib.rs` + `commands.rs` | ALTA | `optimization_cancel_flag` separado: walk-forward y monte-carlo usan flag distinto al backtest |
| `commands.rs` | MEDIA | `preview_data`: LazyFrame + `.limit()` en vez de cargar todo el Parquet en memoria |
| `engine/optimizer.rs` | MEDIA | `ParameterRange.rule_index`: guard `< 0` antes de cast `as usize` |
| `models/result.rs` + `engine/optimizer.rs` | MEDIA | `OptimizationResult.win_rate_pct` añadido; `extract_objective_from_result` devuelve valor real |
| `engine/metrics.rs` | MEDIA | Breakeven trades resetean streaks consecutivos (nuevo branch `else` en `calculate_consecutive`) |
| `engine/optimizer.rs` | MEDIA | GA stagnation: threshold relativo `prev_best.abs() * 1e-4` en vez de `f64::EPSILON` |
| `engine/strategy.rs` | PERFORMANCE | Short-circuit en `evaluate_rules`: AND+false y OR+true hacen `continue` sin evaluar |
| `engine/metrics.rs` | PERFORMANCE | `equity_to_daily_returns` cacheado — llamado una vez, resultado compartido para Sharpe/Sortino/Omega |
| `engine/optimizer.rs` | PERFORMANCE | `best_so_far`: Mutex reemplazado por `AtomicU64` con CAS loop (lock-free) |
| `models/candle.rs` | SIMPLIFICACIÓN | Struct `TickData` eliminado (obsoleto, reemplazado por `TickColumns`) |
| `engine/strategy.rs` | SIMPLIFICACIÓN | `max_lookback()`: +1 solo cuando hay comparador CrossAbove/CrossBelow |

### Limitaciones de realismo conocidas (features futuras)
- **Factor de anualización**: Los 252 días base son correctos para equities, pero crypto debería usar 365. Actualmente, `bars_per_day()` asume 24h para todos los timeframes — para acciones (8h/día) o forex (23.5h/día) los retornos anualizados quedan levemente inflados.
- **Gap risk en modo barra**: En modo barra, SL/TP siempre se ejecutan al precio exacto. Solo el modo tick simulando con Dukascopy respeta gaps reales.
- **Sharpe sobre retornos por trade**: El estándar académico prefiere retornos sobre series de tiempo (daily equity returns). La implementación actual (per-trade returns × annualization_factor) es válida pero menos robusta para estrategias con frecuencia muy variable.
- **Sin múltiples posiciones simultáneas**: Una posición a la vez (v1 deliberado).
- **CrossAbove/CrossBelow usa `prev_left <= prev_right`**: Si ambos valores son iguales un período y luego uno sube, cuenta como cruce. La definición estricta usaría `<`.

### Métricas adicionales a agregar en futuras fases
- K-Ratio (consistencia de la equity curve: pendiente/ruido)
- Omega Ratio (sin asumir distribución normal)
- Tabla de retornos mensuales % (año × mes, colores verde/rojo)
- Análisis de slippage total
- Distribución de P&L por día de semana
- Monte Carlo sobre los trades históricos (aleatorizar orden → distribución de outcomes)

### Cache de indicadores en optimización (pendiente de implementar)
El CLAUDE.md lo menciona como goal de performance pero NO está implementado aún. En cada combinación de Grid Search se recalculan todos los indicadores desde cero. Implementar `HashMap<(IndicatorKey, params_hash), Vec<f64>>` en el loop del optimizador puede dar 5-20x speedup.