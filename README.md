<!-- SPDX-License-Identifier: GPL-3.0-only -->

<div align="center">
  <img src="data/icons/hicolor/scalable/apps/io.github.cheviiot.alcove.svg" width="128" alt="Значок Alcove">
  <h1>Alcove</h1>
  <p><strong>Любой сайт — отдельное приложение.</strong></p>
  <p>
    <a href="https://github.com/Cheviiot/Alcove/actions/workflows/ci.yml"><img alt="Сборка" src="https://img.shields.io/github/actions/workflow/status/Cheviiot/Alcove/ci.yml?branch=main&amp;style=flat-square&amp;label=%D1%81%D0%B1%D0%BE%D1%80%D0%BA%D0%B0"></a>
    <a href="COPYING"><img alt="Лицензия GPL-3.0-only" src="https://img.shields.io/badge/%D0%BB%D0%B8%D1%86%D0%B5%D0%BD%D0%B7%D0%B8%D1%8F-GPL--3.0--only-6f7782?style=flat-square"></a>
    <img alt="GTK 4 и libadwaita" src="https://img.shields.io/badge/GTK%204-libadwaita-4a86cf?style=flat-square">
    <img alt="Flatpak" src="https://img.shields.io/badge/%D0%BF%D0%B0%D0%BA%D0%B5%D1%82-Flatpak-1c1d22?style=flat-square">
  </p>
  <p><strong>Русский</strong> · <a href="README.en.md">English</a></p>
</div>

Alcove превращает сайт в приложение GNOME: собственное окно, профиль, cookie,
кэш и разрешения. Два аккаунта одного сервиса работают рядом и не знают друг о
друге. Приложение запускается из меню и не зависит от браузера.

## Возможности

- Два аккаунта одного сервиса одновременно — без инкогнито и второго браузера.
- Своя иконка в меню и своё место в переключателе окон.
- Камера, микрофон и уведомления выдаются сайту, а не браузеру целиком.
- Навигация, прокси, фоновый режим, фильтры содержимого и user-agent — по сайтам.
- Профиль выгружается в шифрованный архив и переносится на другую машину.
- Доступ к системе только через порталы XDG, без широких разрешений Flatpak.

## Два движка

По умолчанию работает **WebKitGTK**. **Chromium** — дополнение Flatpak для
сайтов, которым WebKitGTK не хватает: ставится отдельно, движок не меняется без
подтверждения, профили и сессии двух движков не смешиваются.

DRM и Widevine, расширения браузера и обход антибот-защиты не поддерживаются.

## Установка

```sh
flatpak install --user https://cheviiot.github.io/Alcove/alcove.flatpakref
```

Выпусков пока нет — до первого приложение собирается из исходников, см.
[CONTRIBUTING.md](docs/CONTRIBUTING.md).

## Приватность

Ни статистики, ни обращений к сторонним службам, ни отправки открываемых
адресов. Наружу Alcove ходит только на сами сайты, которые вы открыли.

## Участие

[Issues](https://github.com/Cheviiot/Alcove/issues) ·
[CONTRIBUTING.md](docs/CONTRIBUTING.md) ·
[Интерфейс](docs/gnome-hig.md) ·
[Модель угроз](docs/threat-model.md) ·
[История изменений](docs/CHANGELOG.md)

## Происхождение

Независимое продолжение [Spider](https://github.com/Zaedus/spider) от коммита
`dcf9d1080ce2bbd89c342b4766a94e18aaecf660`, созданного Zaedus при участии
Cameron Radmore. История Git начинается заново; авторство унаследованного кода
записано в [AUTHORS.md](docs/AUTHORS.md). Прежние авторы в Alcove не участвуют.

Лицензия GPL-3.0-only — [NOTICE](docs/NOTICE), [COPYING](COPYING).
