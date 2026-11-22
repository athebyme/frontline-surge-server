# Защита ветки main

Конфигурация для импорта находится в [.github/main-ruleset.json](../.github/main-ruleset.json). Защиту включает владелец репозитория в настройках GitHub.

1. Открой pull request в `main` и дождись успешного задания GitHub Actions `server-test-build`. При настройке GitHub использует проверки, которые успешно выполнялись в этом репозитории за последние семь дней.
2. Открой [настройки правил репозитория](https://github.com/athebyme/frontline-surge-server/settings/rules). Если правило **main requires PR and CI** уже существует, открой его для сверки. Для первого включения выбери **New ruleset → Import a ruleset** и загрузи файл `.github/main-ruleset.json` из этой копии репозитория.
3. Проверь режим **Active**, цель `refs/heads/main` и пустой список **Bypass**. Для слияния требуется pull request с **0** обязательных одобрений и успешное задание GitHub Actions `server-test-build`. Включена блокировка принудительной перезаписи истории. Нажми **Create** при импорте или **Save changes** при редактировании.
4. Снова открой правило и проверь сохранённый статус **Active**. В следующем pull request целевая ветка должна быть `main`, а задание `server-test-build` должно отображаться как обязательное. Пока оно выполняется или завершается ошибкой, GitHub блокирует слияние.

[Импорт правил в документации GitHub](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/managing-rulesets-for-a-repository#importing-a-ruleset) · [Проверка обязательных статусов](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)
