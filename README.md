# Little a Map

Players can have little a map—if they've surveyed the area in-game. This tool
renders a composite of existing map items with the goal of minimizing external
effects on survival gameplay.

Design goals:

- Preserve the feeling that the world is large and mysterious.
- Avoid introducing unearned navigational aids.
- Facilitate sharing of geographic knowledge between players, as a treat.

## Usage

Render a statically servable slippy map from a game save:

```console
$ little-a-map --output '/var/www/html' '/opt/mscs/worlds/example'
Rendered 341 map items onto 1862 tiles in 0.34s.
```

## Acknowledgements

_Little a Map_ is inspired by _[Papyri]_ by [Jason Green].

[jason green]: https://jason.green.io/
[papyri]: https://github.com/jason-green-io/papyri
