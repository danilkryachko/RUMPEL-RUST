# Технический анализ проекта Rumpel Rust

## 1. Архитектура ядра Rust (Codex)
- Модульная многокритовая архитектура ECS.
- Декларативный RON-реестр (assets/blocks/base.ron).
- Оптимизированный macOS-лейаут сборщика (sccache & ld64.lld).

## 2. Lua API и Моддинг (Antigravity)
- Safe bounded API rumpel_world::world_gen.
- IDE-stub автодополнение в api_stub.lua.

## 3. Графика и Рендеринг (gemini)
- Рендеринг surface_streaming с VoxelQuadMaterial.
- Алгоритм greedy greedy quads для экономии полигонов.

## 4. Контроль качества (Команда)
- Проект на 100% соответствует правилам конституции AGENTS.md.