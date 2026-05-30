#!/bin/bash
# Скрипт для запуска release-сборки без Bevy dynamic linking.

set -euo pipefail

echo "Компиляция и запуск release-сборки игры..."
cargo run -p rumpel_client --release
