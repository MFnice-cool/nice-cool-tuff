# TuffBox Admin

Локальная админ-панель над community Supabase:

- **Online chart** — launcher online + in-Minecraft concurrent over time
- история сессий (launcher / game)
- **Crash Votes moderation** — Accept/Reject фиксов после голосов игроков
- топ companions для мода (`partners_for_mod`)

## Online chart

Каждый heartbeat лаунчера пишет минутный sample. Launch Minecraft → `game` session; exit → duration.

В админке вкладка Online: график 6h / 24h / 3d / 7d.

## Запуск

Открой файл в браузере:

```text
apps/tuffbox-admin/index.html
```

Или локальный сервер:

```bash
npx --yes serve apps/tuffbox-admin
```

## Crash Votes moderation

1. Вкладка **Crash Votes**
2. Введи admin secret (хранится в `admin_config.moderation_secret`; не коммить в git)
3. Load queue → **Accept → saved** или **Reject**

Сменить секрет:

```sql
update public.admin_config
set value = 'your-long-secret-min-6-chars', updated_at = now()
where key = 'moderation_secret';
```

Голоса игроков (Keep/Discard) только копят счётчики. Финальный статус ставит админ.

Ключ publishable уже встроен (как в лаунчере). Direct table writes закрыты RLS; панель ходит только в RPC.
