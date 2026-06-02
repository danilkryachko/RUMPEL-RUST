# AI Development Guide (Конституция Проекта)

**ВНИМАНИЕ ВСЕМ ИИ-АГЕНТАМ:** Если вы читаете это, вы работаете над проектом "Rumpel Rust" (Voxel Game).
Владелец проекта (Пользователь) выступает в роли **Продюсера**. Он пишет высокоуровневые требования и НЕ читает код. Ваша задача — принимать решения, распределять задачи между агентами и поддерживать идеальную чистоту кода.

Для автоматического подхвата этих правил в Codex верхнеуровневый файл `AGENTS.md` дублирует обязательный workflow и маппинг проектных ролей на реальные subagent-типы текущей среды.

## 1. Базовые правила
1. **Никакого ИИ-слопа (спагетти-кода).** Код должен быть строгим, модульным и читаемым.
2. **Всегда используйте RAG-память.** Прежде чем писать код, загляните в папку `.ai_memory/`. Там лежат Architecture Decision Records (ADR), логи багов и контекст. Чтобы искать по памяти, используйте `grep_search` или просматривайте файлы напрямую.
3. **Строгий Rust:** Мы используем `#![deny(clippy::pedantic)]`. Никаких предупреждений быть не должно.
4. **Строгая типизация координат:** ЗАПРЕЩЕНО использовать `Vec3` или `IVec3` для логики блоков. Используйте только `WorldPos`, `ChunkPos` и `LocalBlockPos` из `coordinates.rs`.
5. **Data-Driven блоки:** ЗАПРЕЩЕНО хардкодить свойства блоков в Rust-коде. Все новые блоки добавляются только через редактирование файла `assets/blocks/base.ron`. Код должен работать с любыми блоками из реестра.

## 2. Архитектура
* **Движок:** Bevy Engine (целевая версия: 0.18.x).
* **Модульность (Cargo Workspaces):** Проект разбит на независимые крейты в папке `crates/` (`rumpel_coords`, `rumpel_blocks`, `rumpel_world`, `rumpel_render`, `rumpel_player`, `rumpel_client`). ЗАПРЕЩЕНО писать весь код в одном файле. Каждая новая крупная система (Инвентарь, Сеть, Звук) должна создаваться как отдельный Crate через `cargo new crates/rumpel_X --lib`.
* **Быстрая линковка (Прелюдия):** Для импорта общих структур (координаты, чанки, реестры) во всех модулях обязательно используйте `use rumpel_prelude::*;`. Не импортируйте крейты напрямую, если они есть в прелюдии.
* **ECS (Entity Component System):** Строго соблюдайте паттерн ECS. Не используйте ООП. Логика пишется только в System (`fn my_system(query: Query<...>)`), данные хранятся только в Component и Resource.`.
  * Никогда не смешивайте их. Не создавайте гигантских систем типа "god_object".

## 3. Мультиагентная Система
Главный ИИ, который общается с Продюсером, выполняет роль **Геймдиректора**.
Если задача сложная, Геймдиректор **ОБЯЗАН** делегировать ее специализированным субагентам:
* `engine_architect`: Сложные вычисления, оптимизация, математика вокселей, Greedy Meshing, многопоточность.
* `gameplay_coder`: Логика игрока, UI, взаимодействие с миром.
* `code_reviewer`: Тестирование, проверка `clippy`, запуск `cargo check`.
* **НОВЫЕ ОТДЕЛЫ (AAA Studio):**
  * `game_designer`: Ведет `GDD.md` (документацию) и `BOARD.md` (доску задач), проектирует фичи.
  * `art_director`: Создает концепты (`generate_image`), текстуры, руководит папкой `assets/`.

В **Cursor** проектные субагенты лежат в `.cursor/agents/` (`engine-architect`, `gameplay-coder`, `code-reviewer`, `game-designer`, `art-director`). Вызов: `/code-reviewer …` в Agent-чате редактора или делегирование главному агенту. Проверка наличия — файлы в `.cursor/agents/`, не обязательно список в Settings (в Cursor 3.x UI индексация субагентов часто пустая при рабочих slash-командах).

В Codex-средах, где доступны только типы `explorer`, `worker` и `default`, используйте следующий маппинг:
* `engine_architect` и `gameplay_coder` -> `worker`.
* `code_reviewer` -> `worker` для независимой проверки или локальные `cargo`-проверки для маленьких задач.
* Read-only исследование кодовой базы -> `explorer`.
* Если subagent-инструменты недоступны, продолжайте локально и кратко сообщите об ограничении.

## 4. Протокол Codex + Antigravity
Проект может разрабатываться совместно агентами Codex и Antigravity. Это протокол сотрудничества, а не разрешение на хаотичные параллельные правки.

* **Продюсер:** Пользователь владеет направлением продукта и финальным одобрением.
* **Codex:** По умолчанию отвечает за финальную интеграцию, согласование архитектуры и проверку результата, если Продюсер явно не назначил другого владельца.
* **Antigravity:** Может предлагать решения, реализовывать ограниченные задачи или делать ревью при явном scope и file ownership.
* **Перед правками:** Каждый агент обязан проверить текущий `git diff`/working tree и работать с уже существующими изменениями, а не поверх воображаемого состояния проекта.
* **Запрет на перетирание:** Агент не откатывает, не переписывает и не форматирует чужие изменения без явной команды Продюсера.
* **Параллельная работа:** Перед стартом каждая задача должна назвать scope, файлы для редактирования, файлы вне зоны ответственности и ожидаемую команду проверки.
* **Общие файлы:** `Cargo.toml`, `AGENTS.md`, `AI_DEVELOPMENT_GUIDE.md`, `GDD.md`, `BOARD.md` и границы core-crate'ов требуют особой осторожности: сначала изучить diff, затем править минимально.
* **Публикация:** `commit` и `push` разрешены только по явной команде Продюсера.
* **Финальная проверка:** Перед публикацией интеграционный владелец запускает `just verify` или эквивалентные Cargo-проверки.

## 5. Качество Кода (Zero TODOs Policy)
* **Доводить до конца:** ЗАПРЕЩЕНО использовать заглушки, писать комментарии `// TODO`, `// FIXME` или оставлять пустые функции на будущее.
* **Делать сразу:** Если вы беретесь за фичу, она должна быть реализована от начала и до конца на 100% в рамках текущей задачи, либо не начинайте ее вообще. Любой написанный код должен быть готов к релизу (Production-ready).

## 6. Контроль Версий (Git / GitHub)
* **Безопасные коммиты:** Агент НЕ делает `commit` или `push` без явной команды Продюсера. Перед коммитом агент обязан показать scope изменений и убедиться, что не захватывает чужие правки.
* **Формат коммитов:** Используйте Conventional Commits (`feat: ...`, `fix: ...`, `refactor: ...`, `docs: ...`, `chore: ...`).
* **Проверки перед публикацией:** Перед `push` должны проходить `just verify` или эквивалентный набор `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`.
* **Changelog:** Для релизных изменений используйте `git-cliff` и Conventional Commits. Команда `just changelog` обновляет `CHANGELOG.md`.

## 7. Кэш Сборки (sccache)
* **Обязательный wrapper:** Локальные Rust-сборки должны использовать `.cargo/config.toml`, где настроен `sccache` как `rustc-wrapper`.
* **Не использовать ccache для Rust:** `ccache` подходит для C/C++, но не заменяет `sccache` для `rustc`.
* **Быстрый macOS linker:** На рабочей macOS-машине проекта включен Homebrew LLVM linker `/opt/homebrew/bin/ld64.lld` для `aarch64-apple-darwin`. Не удаляйте этот флаг без измеримого регресса или несовместимости окружения.
* **Повторяемые проверки:** Для долгих повторных проверок используйте `just check-cached`; для запуска клиента с отключенным incremental cache используйте `just dev-cached`.
* **Диагностика:** Для проверки кэша используйте `just sccache-stats` или `sccache --show-stats`.
* **Если sccache отсутствует:** В локальной macOS-среде установите его через `brew install sccache`, если это разрешено окружением.

## 8. Lua API и Worldgen
* **Startup-моды:** Обычные Lua-моды в `assets/mods/*.lua` регистрируют блоки, мобов, поведения, частицы и runtime-события.
* **Worldgen-мод:** `assets/mods/world_gen.lua` не является startup-модом. Он запускается как bounded post-pass через `rumpel_world::world_gen` и получает только `get_block`, `set_block`, `get_height`, `Chunk` и безопасные no-op/intent функции.
* **IDE typing:** `assets/mods/api_stub.lua` и `assets/mods/.luarc.json` являются служебными файлами для Lua Language Server. Runtime loader обязан пропускать `api_stub.lua`, чтобы не подменять настоящие Rust API.
* **Async safety:** Не используйте persistent gameplay `LuaRuntime` внутри render/mesh async tasks. Для генерации чанков используйте изолированный Lua VM или заранее валидированный worldgen context.

## 9. Surface Rendering и FPS
* **Surface path по умолчанию:** Основной клиент использует `rumpel_render::surface_streaming`; `RUMPEL_COMPUTE_PROTOTYPE=1` оставлен только для измеряемого GPU compute prototype.
* **Материал terrain:** Для streamed heightmap terrain используйте `VoxelQuadMaterial` и `assets/shaders/voxel_quads.wgsl`. Не возвращайте surface terrain на per-chunk `StandardMaterial`, иначе потеряются texture-array atlas, repeat UV и culling-настройки.
* **Greedy merge:** Surface heightmap уже склеивает top faces и side walls в merged quads. Новые оптимизации должны сохранять повтор текстуры через `ATTRIBUTE_VOXEL_REPEAT_UV` и tile ID через `ATTRIBUTE_VOXEL_TILE`.
* **Проверка FPS:** Для измерений используйте автопролет, например `RUST_LOG=wgpu=error,bevy_asset=error RUMPEL_PROFILE_SECONDS=8 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=2 cargo run -p rumpel_client`. Смотрите не только FPS, но и `surface_sample_vertices`, `surface_sample_indices`, build/upload/stream timings.
* **Дальность прорисовки:** Не уменьшайте `VIEW_RADIUS_CHUNKS` ради FPS без прямой команды Продюсера. Цель оптимизации — сохранять дальность и снижать стоимость mesh/render path.
* **Render mode:** Для повторяемых замеров явно задавайте `RUMPEL_RENDER_MODE=surface` или `RUMPEL_RENDER_MODE=compute`. Старый `RUMPEL_COMPUTE_PROTOTYPE=1` остается алиасом для compute prototype, но новые команды должны использовать `RUMPEL_RENDER_MODE`.
* **Compute direct render:** В compute mode direct terrain render включен по умолчанию через `RUMPEL_COMPUTE_DIRECT_RENDER=1`: compute mesher пишет в GPU-owned arena buffers, а render node рисует их напрямую без Bevy mesh copy-back и без per-chunk terrain bind groups. По умолчанию direct renderer использует `multi_draw_indirect` при поддержке `INDIRECT_FIRST_INSTANCE` и GPU frustum cull pass через `RUMPEL_COMPUTE_DIRECT_GPU_CULL=1`: cull pass пишет per-view indirect command buffer и зануляет невидимые chunks, так что Metal/macOS не требует `MULTI_DRAW_INDIRECT_COUNT`. На backend-ах с `MULTI_DRAW_INDIRECT_COUNT` включен feature-gated compact path (`RUMPEL_COMPUTE_DIRECT_GPU_CULL_COMPACT=1`): shader atomically appends visible commands и render node вызывает `multi_draw_indirect_count`; `RUMPEL_COMPUTE_DIRECT_GPU_CULL_COMPACT=0` оставляет fixed-count path для A/B. `RUMPEL_COMPUTE_DIRECT_GPU_CULL=0` отключает cull; `RUMPEL_COMPUTE_DIRECT_MULTI_INDIRECT=0` оставляет loop-based `draw_indirect` fallback, а `RUMPEL_COMPUTE_DIRECT_INDIRECT=0` оставляет direct draw loop fallback.
* **GPU-driven roadmap:** Долгосрочный путь описан в `.ai_memory/ADR-002-gpu-driven-voxel-roadmap.md`. Сначала делаем compute parity slice, затем GPU streaming queue, packed quads/vertex pulling, MDI/GPU culling и только потом far-field SVO/raymarch.

## 10. Использование Долгосрочной Памяти (agentmemory)
Если вы приняли важное архитектурное решение (например, изменили формат хранения чанков), вы **обязаны** использовать инструмент `memory_save` (предоставляемый через MCP `agentmemory`), чтобы сохранить это знание в векторную базу данных проекта. Будущие агенты смогут найти это через `memory_search`. Никогда не заставляйте Продюсера повторять одно и то же дважды.
