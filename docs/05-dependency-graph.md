# 05. Граф зависимостей модов

## Цель

Представить сборку Minecraft как граф, где узлы — моды, loader, Minecraft, Java, libraries, configs и profiles, а рёбра — отношения между ними.

## Типы узлов

```text
MinecraftVersion
Loader
JavaRuntime
Mod
Library
ConfigFile
ScriptFile
ResourcePack
ShaderPack
Profile
```

## Типы рёбер

```text
requires             # обязательная зависимость
optional             # опциональная зависимость
conflicts            # конфликт
breaks_with          # известная несовместимость
replaces             # мод является заменой другого
requires_loader      # требует loader
requires_minecraft   # требует версию Minecraft
requires_java        # требует Java
client_only          # только клиент
server_only          # только сервер
both_sides           # нужен и там, и там
loads_before         # порядок загрузки
loads_after          # порядок загрузки
configured_by        # связан с config-файлом
modified_by_script   # изменяется KubeJS/CraftTweaker
```

## Пример

```text
Oculus ─requires──> Embeddium
Oculus ─requires_loader──> Forge 47.x
Oculus ─requires_minecraft──> Minecraft 1.20.1
Rubidium ─conflicts──> Embeddium
KubeJS ─requires──> Rhino
KubeJS ─configured_by──> kubejs/server_scripts/recipes.js
```

## Статусы узлов

```text
Ok
MissingDependency
VersionMismatch
Conflict
Duplicate
Deprecated
UnknownSide
ClientOnlyInServerProfile
ServerOnlyInClientProfile
UpdateAvailable
RiskyUpdate
LocalOnly
Unresolved
```

## Version constraint

Версии нужно описывать не строкой, а constraint-объектом:

```json
{
  "kind": "range",
  "min": "1.6.0",
  "max": "1.7.0",
  "includeMin": true,
  "includeMax": false
}
```

## Resolver pipeline

```text
1. Load project manifest
2. Load lockfile
3. Fetch metadata for known mods
4. Normalize versions
5. Create graph nodes
6. Create dependency edges
7. Apply compatibility rules
8. Detect missing dependencies
9. Detect conflicts
10. Detect side mismatch
11. Generate diagnostics
12. Generate change plan
```

## Change plan

Resolver не должен сразу менять проект. Он возвращает план:

```json
{
  "summary": "Install missing dependencies for KubeJS",
  "risk": "low",
  "actions": [
    {
      "type": "install_mod",
      "projectId": "rhino",
      "version": "2001.2.2-build.18"
    }
  ],
  "requiresSnapshot": true
}
```

## Почему граф важен

Граф позволяет:

- объяснить, почему мод установлен;
- понять, что сломается при удалении;
- построить server pack;
- найти конфликтующие моды;
- связать crash log с конкретными узлами;
- делать безопасные обновления;
- делать reproducible builds.
