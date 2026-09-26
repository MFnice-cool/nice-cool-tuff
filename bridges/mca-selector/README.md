# Bundled MCA Selector

Querz [MCA Selector](https://github.com/Querz/mcaselector) ships with TuffBox so World Map can open the real editor without a GitHub download.

## Layout

```
prebuilt/
  mcaselector-2.8.jar
  javafx-lib/          # OpenJFX SDK `lib/` (platform-specific)
```

Populate with:

```powershell
powershell -File bridges/mca-selector/fetch-prebuilt.ps1
```

Tauri bundles `prebuilt/` as `mca-selector/` resources. At runtime the launcher prefers this copy and launches:

`java --module-path <javafx-lib> --add-modules ALL-MODULE-PATH -jar mcaselector-2.8.jar`

when the selected JRE has no built-in JavaFX (e.g. GraalVM).
