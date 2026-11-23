# Как запустить выделенный сервер

Сервер Frontline Surge 0.5 работает отдельным процессом и принимает клиентов игрового протокола 5 по TCP. Карта встроена в программу. Для игры нужен клиент той же версии. Матч живёт в памяти процесса. Перезапуск завершает текущую партию.

## Сначала проверить сборку

Для выпуска 0.5 использовались Rust 1.88 и зафиксированные зависимости из Cargo.lock. Из корня репозитория выполни следующие команды.

```sh
cargo test --locked
python3 tools/check_level.py
cargo run --release --locked --bin frontline-server -- --bind 127.0.0.1 --port 8787 --bots 4
```

На этом же компьютере введи в клиенте `127.0.0.1:8787`. После подключения сервер напечатает `Join` и имя бойца. В консоли команда `quit` завершает процесс. На Linux работают также Ctrl+C и SIGTERM. Версию и список параметров покажут `--version` и `--help`.

| Параметр | Что меняет |
|---|---|
| `--bind 0.0.0.0` | Слушать все локальные IPv4-интерфейсы. Для матча на одном ПК выбери `127.0.0.1`. |
| `--port 8787` | Игровой порт TCP. Порт 0 позволяет ОС выбрать свободный порт для теста. |
| `--bots 4` | После входа первого человека добавить четырёх ботов. Всего 12 мест. Допустимы 0–10 ботов, по умолчанию 0. |
| `--round-minutes 120` | Продолжительность раунда от 120 до 180 минут. |
| `--dev-local` | Разрешить действия разработчика при соединении через loopback. Для публичной службы опция выключена. |

## Подготовить комплект для Linux x86-64

Команды рассчитаны на машину сборки Linux x86-64 с Rust и rustup. Результат — автономный сервер с musl, уведомления о зависимостях и шаблон службы. Для повторной сборки достаточно сохранённых в кеше зависимостей и установленной целевой платформы. К Cargo можно добавить `--offline`.

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl --bin frontline-server
mkdir -p dist/linux-x64
install -m 0755 target/x86_64-unknown-linux-musl/release/frontline-server dist/linux-x64/
install -m 0644 notices/THIRD-PARTY-Linux-musl.txt dist/linux-x64/THIRD-PARTY.txt
install -m 0644 deploy/frontline-server.service dist/linux-x64/
(cd dist/linux-x64 && sha256sum frontline-server THIRD-PARTY.txt frontline-server.service > SHA256SUMS)
```

Перенеси каталог `linux-x64` на свой VPS обычным способом, например через SCP. Сверь сумму манифеста или самого бинарника с машиной сборки, если архив передавался отдельно. На VPS открой перенесённый каталог и проверь его содержимое.

```sh
sha256sum -c SHA256SUMS
./frontline-server --version
```

При утрате права исполнения после распаковки выполни `chmod +x frontline-server`. Ожидаемая строка для этой версии — `Frontline Surge 0.5.2 · protocol 5`.

## Установить службу на Debian с systemd

Для установки под `/opt` и регистрации службы нужен административный доступ. Сама игра запускается с временным непривилегированным системным пользователем, которого создаёт systemd. Для нового сервера сначала проверь `ss -ltn` и `systemctl cat frontline-server.service`. Выбери свободный порт и убедись, что имя службы доступно.

Из проверенного каталога `linux-x64` создай отдельное место для сборки. Имя каталога берётся из хеша фактического бинарника.

```sh
base=/opt/frontline-surge
release_id=$(sha256sum frontline-server | cut -d' ' -f1)
(
  set -e
  sudo install -d -m 0755 "$base/releases"
  dest="$base/releases/$release_id"
  if [ -d "$dest" ]; then
    cmp frontline-server "$dest/frontline-server"
    cmp THIRD-PARTY.txt "$dest/THIRD-PARTY.txt"
  else
    stage=$(sudo mktemp -d "$base/releases/.staged-XXXXXXXX")
    trap 'sudo rm -rf -- "$stage"' EXIT
    sudo install -m 0755 frontline-server "$stage/frontline-server"
    sudo install -m 0644 THIRD-PARTY.txt "$stage/THIRD-PARTY.txt"
    sudo chmod 0755 "$stage"
    sudo mv -T "$stage" "$dest"
  fi
)
```

При **первой установке** создай ссылку на версию и зарегистрируй шаблон из комплекта. Команда `ln -s` сообщит ошибку, если уже существует выбранная активная версия. Для существующей службы используй процедуру обновления ниже.

```sh
sudo ln -s "releases/$release_id" "$base/current"
sudo install -m 0644 frontline-server.service /etc/systemd/system/frontline-server.service
sudo systemctl daemon-reload
sudo systemctl enable --now frontline-server.service
systemctl status frontline-server.service --no-pager
journalctl -u frontline-server.service -n 30 --no-pager
```

Шаблон запускает четыре бота и двухчасовые раунды. Параметры можно изменить командами `sudo systemctl edit --full frontline-server.service` и `sudo systemctl daemon-reload`. Согласуй перезапуск перед применением настроек к работающей игре.

Разреши выбранный игровой TCP-порт в сетевых правилах хостинга. Если используется активный UFW, для порта по умолчанию команда — `sudo ufw allow 8787/tcp`. Посмотри состояние через `sudo ufw status`. Для домашнего хоста за NAT потребуется проброс того же TCP-порта на компьютер с сервером. С другого подключения введи в клиенте публичное имя или IP сервера и порт. Появление бойца в матче и строки `Join` в журнале подтверждает доступ извне.

## Обновить или откатить

Сначала собери и проверь новый комплект. Помести его в свой каталог под `releases` по командам выше. Убедись, что новые клиент и сервер используют один протокол. Если хеш совпал с работающей версией, достаточно оставить службу запущенной.

Для установки другой версии дождись согласованной паузы между матчами. Запомни текущую цель ссылки командой `readlink /opt/frontline-surge/current`. Следующий блок выполняется с переменными `base` и `release_id` новой проверенной сборки.

```sh
sudo ln -s "releases/$release_id" "$base/current.next"
sudo mv -Tf "$base/current.next" "$base/current"
sudo systemctl restart frontline-server.service
systemctl status frontline-server.service --no-pager
journalctl -u frontline-server.service -n 30 --no-pager
```

Проверь вход с клиента. Для отката назначь ссылке ранее сохранённую цель теми же командами и перезапусти службу. Предыдущие каталоги в `releases` сохраняются. Откат запускает новый матч.

## Windows и уведомления о зависимостях

На Windows запусти `cargo build --release --locked --bin frontline-server`. Запуск в PowerShell — `.\target\release\frontline-server.exe --port 8787 --bots 4`. Для выпуска сборки GNU выбери target `x86_64-pc-windows-gnu` и соответствующий MinGW-w64. Сервер работает как консольная программа.

Готовые тексты уведомлений поставляются отдельно для [Linux с musl](../notices/THIRD-PARTY-Linux-musl.txt) и [Windows с MinGW-w64](../notices/THIRD-PARTY-Windows-GNU.txt). В них указаны версии зависимостей из Cargo.lock, лицензии стандартной библиотеки Rust 1.88 и тексты соответствующей платформы. Приложи нужный файл к двоичному выпуску. При смене зависимостей, toolchain или платформы проверь состав поставки и обнови уведомления по использованным компонентам.
