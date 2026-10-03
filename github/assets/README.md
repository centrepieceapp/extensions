# Octicons

[primer.style/octicons](https://primer.style/octicons/), MIT (see `LICENSE`),
from the `@primer/octicons` npm package (v19.38.0). The 16px variants, because
they are drawn for the size the rows show them at; the `-16` suffix is dropped
on the way in so the file names match the icon names on the site.

```sh
npm pack @primer/octicons && tar xzf primer-octicons-*.tgz
cp package/build/svg/<name>-16.svg extensions/github/assets/<name>.svg
```

One name differs from the site: "view on GitHub" uses `browser`, Octicons having
no icon called `browse`.

Centrepiece draws extension SVGs as an alpha mask tinted with the theme, so every
file has to be single-colour line art.
