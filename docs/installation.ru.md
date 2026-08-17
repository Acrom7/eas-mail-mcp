# Установка EAS Mail MCP

Публичный `v0.1.2` распространяется только как исходный код. Готовый bundle для
реального сервера должен предоставить оператор, который собрал его с локальным
build-time профилем и опубликовал SHA-256.

## Передача одного файла ИИ-агенту

`eas-mail-mcp-0.1.2-macos-handoff.tar.gz` содержит оба macOS-бинарника,
installer, внутренний `SHA256SUMS` и `INSTALL-FOR-AI-AGENT.md`. Коллега скачивает
его локально и передаёт coding agent путь к файлу с просьбой выполнить инструкцию
из архива. Загружать корпоративный бинарник во внешний чат не требуется.

Внешний `.tar.gz.sha256` можно публиковать отдельно через доверенный канал для
проверки архива до распаковки. Для пилотной передачи одного файла installer всё
равно проверит каждый распакованный payload по внутреннему manifest. Внутренний
manifest обнаруживает повреждение, но сам по себе не подтверждает автора bundle.

## Проверка bundle

Выберите архив под архитектуру Mac: `aarch64-apple-darwin` для Apple Silicon или
`x86_64-apple-darwin` для Intel. До распаковки проверьте внешний hash:

```bash
shasum -a 256 -c eas-mail-mcp-0.1.2-<target>.tar.gz.sha256
tar -xzf eas-mail-mcp-0.1.2-<target>.tar.gz
cd eas-mail-mcp-0.1.2-<target>
cat BUILD-METADATA.json
./install.sh
```

Установщик повторно проверяет файлы по `SHA256SUMS`, сверяет архитектуру и
создаёт `~/.local/bin/eas-mail-mcp`. `sudo` не используется. Для unsigned bundle
с quarantine снятие атрибута требует отдельного подтверждения после проверки
hash.

## Настройка аккаунтов

Доступные ID профилей зависят от конкретной сборки. Посмотреть metadata сборки и
запустить мастер:

```bash
eas-mail-mcp --version --verbose
eas-mail-mcp setup
eas-mail-mcp account list
eas-mail-mcp doctor
```

Пароль вводится в закрытом prompt и сохраняется только в macOS Keychain. Запись
по умолчанию выключена и включается отдельно после read-only проверки:

```bash
eas-mail-mcp account set-writes <account-id> on
```

## Подключение клиентов

```bash
eas-mail-mcp client configure codex
eas-mail-mcp client configure claude
eas-mail-mcp client configure opencode
```

Перед изменением пользовательских конфигов создаётся backup. Неизвестная версия
клиента отклоняется без изменений. Для write tools настраивается режим `ask`, но
он является подтверждением интерфейса, а не аутентификацией.

## Удаление

По умолчанию данные пользователя и Keychain сохраняются:

```bash
~/.local/lib/eas-mail-mcp/0.1.2/share/uninstall.sh
```

Удалить также client entries и локальные данные:

```bash
~/.local/lib/eas-mail-mcp/0.1.2/share/uninstall.sh \
  --clients codex,claude,opencode --delete-data
```
