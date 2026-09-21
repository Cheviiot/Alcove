<!-- SPDX-License-Identifier: GPL-3.0-only -->

<div align="center">
  <img src="data/icons/hicolor/scalable/apps/io.github.cheviiot.alcove.svg" width="128" alt="Значок Alcove">
  <h1>Alcove</h1>
  <p><strong>Любой сайт — отдельное приложение.</strong></p>
  <p>
    <a href="https://github.com/Cheviiot/Alcove/releases/latest"><img alt="Выпуск" src="https://img.shields.io/github/v/release/Cheviiot/Alcove?style=flat-square&amp;label=%D0%B2%D1%8B%D0%BF%D1%83%D1%81%D0%BA&amp;color=4a86cf"></a>
    <a href="https://github.com/Cheviiot/Alcove/actions/workflows/ci.yml"><img alt="Сборка" src="https://img.shields.io/github/actions/workflow/status/Cheviiot/Alcove/ci.yml?branch=main&amp;style=flat-square&amp;label=%D1%81%D0%B1%D0%BE%D1%80%D0%BA%D0%B0"></a>
    <a href="COPYING"><img alt="Лицензия GPL-3.0-only" src="https://img.shields.io/badge/%D0%BB%D0%B8%D1%86%D0%B5%D0%BD%D0%B7%D0%B8%D1%8F-GPL--3.0--only-6f7782?style=flat-square"></a>
    <img alt="GTK 4 и libadwaita" src="https://img.shields.io/badge/GTK%204-libadwaita-4a86cf?style=flat-square">
    <img alt="Flatpak" src="https://img.shields.io/badge/%D0%BF%D0%B0%D0%BA%D0%B5%D1%82-Flatpak-1c1d22?style=flat-square">
  </p>
  <p><strong>Русский</strong> · <a href="docs/README.en.md">English</a></p>
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

## Установка

```sh
flatpak install --user https://cheviiot.github.io/Alcove/alcove.flatpakref
```

Загрузка — 5 МБ, на диске 13 МБ. Нужен Flatpak и среда `org.gnome.Platform`
версии 50: если её ещё нет, Flatpak поставит её с Flathub сам.

Готовые пакеты и файлы установки лежат на
[странице проекта](https://cheviiot.github.io/Alcove/) и в
[выпусках](https://github.com/Cheviiot/Alcove/releases). Репозиторий и все
сборки подписаны ключом `FA64 0607 BEBF D82E 61EF 72EF 33AD DA41 5AF7 FE09`.

Собран для x86_64 и aarch64. Рассчитан на GNOME; на других рабочих столах
работает настолько, насколько там реализованы порталы XDG.

## Как пользоваться

1. Нажмите «плюс» в заголовке и введите адрес сайта.
2. «Далее» — Alcove сходит на сайт за его именем и иконкой. Ждать не
   обязательно: можно прервать и ввести всё вручную.
3. «Создать» — система спросит подтверждение, и приложение появится в меню.

Дальше каждое приложение настраивается отдельно: разрешения, навигация,
прокси, фоновый режим, фильтры содержимого, выбор движка.

## Два движка

По умолчанию работает **WebKitGTK** — он уже внутри, ставить нечего.

**Chromium** — для сайтов, которым WebKitGTK не хватает. Ставится отдельно и
только по вашему решению:

```sh
flatpak install --user https://cheviiot.github.io/Alcove/alcove-chromium-native.flatpakref
```

150 МБ загрузки, 364 МБ на диске, только x86_64 — на aarch64 остаётся один
WebKitGTK. После установки Alcove нужно закрыть и открыть заново. Движок не
меняется без подтверждения, профили и сессии двух движков не смешиваются.

DRM и Widevine, расширения браузера и обход антибот-защиты не поддерживаются.

## Приватность

Ни статистики, ни обращений к сторонним службам, ни отправки открываемых
адресов. Наружу Alcove ходит только на сами сайты, которые вы открыли.

Разрешения Flatpak — сеть, звук и вывод на экран, и ничего больше:

```
shared=network;ipc;  sockets=wayland;pulseaudio;fallback-x11;  devices=dri;
```

Файлы, ярлыки, загрузки и фоновый режим идут через порталы XDG, каждый раз с
вашего подтверждения. Подробности — в [модели угроз](docs/threat-model.md).

## Сборка из исходников

Сборка, проверки и раскладка репозитория описаны в
[CONTRIBUTING.md](docs/CONTRIBUTING.md).

## Участие

[Issues](https://github.com/Cheviiot/Alcove/issues) ·
[CONTRIBUTING.md](docs/CONTRIBUTING.md) ·
[Интерфейс](docs/interface.md) ·
[Движок Chromium](docs/chromium-engine.md) ·
[Модель угроз](docs/threat-model.md) ·
[История изменений](docs/CHANGELOG.md)

## Происхождение

Независимое продолжение [Spider](https://github.com/Zaedus/spider) от коммита
`dcf9d1080ce2bbd89c342b4766a94e18aaecf660`, созданного Zaedus при участии
Cameron Radmore. История Git начинается заново; авторство унаследованного кода
записано в [AUTHORS.md](docs/AUTHORS.md). Прежние авторы в Alcove не участвуют.

Лицензия GPL-3.0-only — [NOTICE](docs/NOTICE), [COPYING](COPYING).
