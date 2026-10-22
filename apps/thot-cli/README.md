# thot — CLI

CLI para la primera funcionalidad de Thot: importar estudiantes desde CSV y
consultar los estudiantes guardados por `thotd`.

```text
thot [-h servidor] [-p puerto] import students <fichero.csv>
thot [-h servidor] [-p puerto] get students
thot [-h servidor] [-p puerto] get students csv
```

El servidor predeterminado es `localhost:8765`. La configuración se lee en este
orden: opciones de línea de comandos, `./thot-config.yaml`,
`./.thot-config.yaml` y `~/.config/thot/thot-config.yaml`. Cada archivo acepta
claves YAML escalares `host` (o `server`) y `port`.

La importación reconoce nombres de columna en catalán, castellano e inglés,
incluidos `Nom`/`Nombre`/`Name` y `Adreça electrònica`/`Email`/`e-mail`/`mail`.
Las columnas opcionales conocidas se envían al motor; las desconocidas se
ignoran. Una fila con distinto número de campos CSV se omite y se informa por
stderr. Los registros con datos incompletos se envían a `thotd`, que devuelve
los motivos de rechazo en su respuesta JSON.

`get students` muestra una tabla; `get students csv` escribe un CSV con todos
los estudiantes devueltos por el motor.
