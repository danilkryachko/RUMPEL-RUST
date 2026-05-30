# Доска Задач (Kanban Board)

Добро пожаловать в трекер задач проекта **RUMPEL RUST**.
Здесь агенты отслеживают прогресс по задачам.

## 📝 Backlog (Бэклог - Ожидает выполнения)
- [ ] Оптимизация мешинга (Greedy Meshing)
- [ ] Генерация мира с помощью шума Перлина (горы, долины)
- [ ] Механика установки и разрушения блоков
- [ ] Материалы/текстуры `bevy_voxel_world` из `BlockRegistry` вместо временного index mapping
- [ ] Адаптер редактирования блоков через `WorldPos`/`LocalBlockPos` поверх `VoxelWorld`
- [ ] Lua API для событий блоков (`on_block_break`, `on_block_place`)
- [ ] Lua API для рецептов и предметов
- [ ] Проработка базового GDD (Крафт, список руд)
- [ ] Подбор текстур (Art Department)

## ⏳ In Progress (В работе)
- [ ] Настройка пайплайна Большой Студии (CI/CD, GitHub, RAG update)

## 🔍 In Review (На проверке)
- [ ] Проверка FPS при быстром полете после async mesh streaming

## ✅ Done (Готово)
- [x] Инициализация проекта на Rust + Bevy
- [x] Настройка базового FPS-управления и камеры
- [x] Генерация тестового чанка (плоский мир)
- [x] Создание структуры RAG-памяти
- [x] Регистрация агентов (Геймдиректор, Программист, QA)
- [x] Базовый Lua-моддинг через `mlua` и `register_block`
- [x] Первичная интеграция `bevy_voxel_world` как runtime backend для стриминга и мешинга мира
- [x] Surface-aware streaming чанков поверхности вместо 3D-сферы вокруг игрока
- [x] Async mesh generation для чанков с лимитом GPU upload на кадр
- [x] Автоматический profiling/autopilot run клиента для проверки FPS в движении
- [x] LOD чанков без урезания дальности прорисовки мира
- [x] High-FPS режим: no-vsync, continuous update loop, unlit terrain, MSAA off
