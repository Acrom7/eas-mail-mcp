# Установка EAS Mail MCP

Основной сценарий установки на macOS: npm-пакет и интерактивный мастер. Вводить
пароль в MCP-конфиг, `.env` или аргументы команды не нужно.

Полная англоязычная версия инструкции: [Getting started](getting-started.md).

## Требования

- macOS 14+ на Apple Silicon или Intel;
- Node.js 18+ и npm;
- Exchange ActiveSync 14.1 с Basic Auth поверх TLS;
- доступ к необходимой корпоративной сети/VPN и доверенному CA.

Windows и Linux в `0.2.0` не поддерживаются.

## Установка

```bash
npm install -g eas-mail-mcp@latest
eas-mail-mcp --version
eas-mail-mcp native-path
```

Npm выбирает нативный пакет под архитектуру Mac и не запускает `install` или
`postinstall` scripts. Node.js нужен для установки и CLI-launcher, но MCP-клиент
подключается напрямую к Rust-бинарнику, поэтому Node.js не остаётся в рабочем
процессе.

## Мастер настройки

```bash
eas-mail-mcp setup
```

На первом запуске мастер:

1. Импортирует готовый endpoint-профиль или создаёт его вручную.
2. Запрашивает email, логин в формате профиля и скрытый пароль.
3. Проверяет TLS, авторизацию, EAS 14.1, Provision policy и FolderSync.
4. Сохраняет аккаунт и Keychain-данные только после успешной проверки.
5. Отдельно предлагает включить write-доступ, который по умолчанию выключен.
6. Позволяет добавить следующие аккаунты.
7. Настраивает найденные Codex, Claude Code и OpenCode с backup/rollback.
8. Запускает обезличенный `doctor`.

При ошибке можно исправить профиль, email, логин или пароль и повторить
проверку. Неуспешные credentials не сохраняются.

Повторный `eas-mail-mcp setup` открывает меню управления: добавить или исправить
аккаунт, обновить пароль, изменить write-доступ, управлять профилями, настроить
клиентов или запустить диагностику.

## Профиль сервера

Профиль содержит адрес EAS-сервера, разрешённые почтовые домены, формат логина и
режим TLS trust. В нём нет пароля, Device ID, policy key или данных конкретного
аккаунта.

Если коллега или администратор выдал готовый профиль:

```bash
eas-mail-mcp profile validate ./team-profile.toml
eas-mail-mcp profile import ./team-profile.toml
eas-mail-mcp setup
```

Пример профиля schema v2:

```toml
schema_version = 2
bundle_version = "operator-1"

[[profiles]]
id = "work"
display_name = "Work Mail"
host = "mail.example.com"
email_domains = ["example.com"]
device_id_length = 16

[profiles.identity]
mode = "realm_username"
realm = "EXAMPLE"
username_hint = "Short corporate login"

[profiles.trust]
mode = "system"
```

Для `realm_username` пользователь вводит короткий логин, а мастер сам сохраняет
его в виде `REALM\username`. Поведение shell и экранирование обратного слеша на
этот сценарий не влияют. Полная схема, включая профиль с публичным CA:
[Runtime profiles](runtime-profiles.md).

Создать профиль вручную можно через `eas-mail-mcp profile add`.

## Несколько аккаунтов

Добавить второй и последующие аккаунты проще всего через повторный `setup`.
Доступен и отдельный интерактивный сценарий:

```bash
eas-mail-mcp account add
```

Команды обслуживания:

```bash
eas-mail-mcp account list
eas-mail-mcp account update-password <account-id>
eas-mail-mcp account set-writes <account-id> on
eas-mail-mcp account set-writes <account-id> off
eas-mail-mcp account remove <account-id>
```

Write-доступ общий для почтовых и календарных мутаций этого аккаунта. После его
включения операция выполняется сразу, когда агент вызывает write tool: отдельной
формы подтверждения внутри MCP нет.

Для автоматизации сохраняются полные флаги и `--password-stdin`. Неполная
non-TTY команда возвращает `INTERACTIVE_REQUIRED`. Пароль нельзя передавать в
аргументах, `.env`, профиле или MCP-конфиге.

## Подключение MCP-клиентов

Мастер предлагает настроить обнаруженные клиенты. Те же действия можно
выполнить отдельно:

```bash
eas-mail-mcp client configure codex
eas-mail-mcp client configure claude
eas-mail-mcp client configure opencode
```

Конфигуратор создаёт backup, записывает прямой путь к Rust-бинарнику и делает
rollback при ошибке. После настройки перезапустите клиент: изменение файла не
закрывает уже запущенные stdio-сессии.

Для ручной настройки получите путь:

```bash
eas-mail-mcp native-path
```

Примеры TOML/JSON-конфигов Codex, Claude Code и OpenCode находятся в разделе
[Manual configuration](getting-started.md#manual-configuration).

## Проверка

```bash
eas-mail-mcp profile list
eas-mail-mcp account list
eas-mail-mcp doctor
```

После перезапуска клиента безопасная MCP-проверка состоит из read-only tools
`accounts_list` и `folders_list`. Не используйте отправку письма или изменение
календаря как smoke-тест без явного запроса владельца аккаунта.

Каждое активное MCP-подключение запускает один `eas-mail-mcp serve`. Поэтому при
нескольких чатах или задачах процессов может быть несколько. Они должны
завершаться вместе со своими stdio-сессиями.

## Где хранятся данные

```text
~/Library/Application Support/EAS Mail MCP/profiles.toml
~/Library/Application Support/EAS Mail MCP/config.toml
```

Пароли, Device IDs, policy state и HMAC key журнала идемпотентности хранятся в
macOS Keychain. Почтовой или календарной базы нет. SQLite содержит только
метаданные write-операций без тем, адресатов и текста. Вложения попадают во
временный кэш только после отдельного скачивания.

## Обновление и удаление

```bash
npm install -g eas-mail-mcp@latest
```

После обновления перезапустите MCP-клиенты.

```bash
npm uninstall -g eas-mail-mcp
```

Npm uninstall намеренно сохраняет локальные профили, аккаунты и Keychain.
Удалите их через CLI до удаления пакета, если настройки не должны оставаться.

## Частые ошибки

| Ошибка | Что проверить |
| --- | --- |
| `AUTH_REQUIRED` | Пароль и формат email/логина, заданный профилем |
| `ACCESS_DENIED` | EAS-доступ, device policy или allowlist у администратора |
| `CONFIG_INVALID` | `profile validate`; не ослабляйте правила endpoint или TLS |
| `INTERACTIVE_REQUIRED` | Запустите TTY-мастер либо передайте все automation-флаги и `--password-stdin` |
| TLS/network error | Нужную сеть/VPN и утверждённый CA; не отключайте проверку TLS |
| MCP не виден | Повторите `client configure`, перезапустите клиент и запустите `doctor` |

## Ограничения

Поддерживаются только HTTPS, EAS 14.1, Basic Auth и стандартный путь
`/Microsoft-Server-ActiveSync`. OAuth, Microsoft Graph, IMAP, нестандартный
endpoint, redirects, TLS bypass и подмена DeviceType не поддерживаются.
Повторяющиеся встречи можно читать, но нельзя изменять целиком или по одному
повтору.
