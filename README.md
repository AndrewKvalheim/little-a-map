[![Demo][demo-badge]][demo]

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
$ little-a-map '/opt/mscs/worlds/example' '/var/www/html'
Rendered 5958 tiles from 492 maps and 28 banners in 0.48s
```

## Acknowledgements

_Little a Map_ is inspired by _[Papyri]_ by [Jason Green].

[demo]: https://andrewkvalheim.github.io/little-a-map/
[demo-badge]: https://img.shields.io/badge/dynamic/json?color=green&label=demo&query=%24.version&url=https%3A%2F%2Fandrewkvalheim.github.io%2Flittle-a-map%2Fbadge.json
[jason green]: https://jason.green.io/
[papyri]: https://github.com/jason-green-io/papyri
