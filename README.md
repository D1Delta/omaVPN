# omavpn 🛡️

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Platform: Arch Linux / Omarchy](https://img.shields.io/badge/Platform-Arch%20Linux%20%7C%20Omarchy-1793D1.svg?logo=arch-linux&logoColor=white)](https://archlinux.org)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-DEA584.svg?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Engines: sing--box + xray--core](https://img.shields.io/badge/Engines-sing--box%20%2B%20xray--core-green.svg)](#архитектура-двойного-ядра-dual-engine)

Нативный, сверхбыстрый консольный и TUI VPN-клиент для **Arch Linux / Omarchy** (Hyprland, Waybar, systemd), написанный на **Rust**. 

Объединяет мощь двух передовых прокси-ядер (**sing-box 1.14+** и **xray-core 26+**) с автоматическим выбором движка, прозрачным TUN-режимом на базе пользовательского стека **gVisor**, поддержкой ссылок **Incy / Happ** с генерацией аппаратных Linux HWID-заголовков и бесшовной интеграцией в строку состояния Waybar.

---

## Оглавление
- [Возможности](#возможности)
- [Как устроен и работает omavpn](#как-устроен-и-работает-omavpn)
  - [1. Архитектура двойного ядра (Dual-Engine)](#1-архитектура-двойного-ядра-dual-engine)
  - [2. Гибридный TUN-мост и защита от routing loop](#2-гибридный-tun-мост-и-защита-от-routing-loop)
  - [3. Сетевой стек gVisor в TUN](#3-сетевой-стек-gvisor-в-tun)
  - [4. Разделение привилегий (Daemon + Client)](#4-разделение-привилегий-daemon--client)
  - [5. Поддержка подписок Incy / Happ и эмуляция HWID](#5-поддержка-подписок-incy--happ-и-эмуляция-hwid)
- [Поддерживаемые протоколы](#поддерживаемые-протоколы)
- [Установка](#установка)
  - [Способ 1. Arch Linux пакет (.pkg.tar.zst)](#способ-1-arch-linux-пакет-pkgtarzst)
  - [Способ 2. Готовый бинарный архив (Tarball)](#способ-2-готовый-бинарный-архив-tarball)
  - [Способ 3. Сборка из исходников](#способ-3-сборка-из-исходников)
- [Использование](#использование)
  - [Интерактивный TUI](#интерактивный-tui)
  - [Управление через CLI](#управление-через-cli)
  - [Интеграция с Waybar](#интеграция-с-waybar)
  - [Горячие клавиши Hyprland](#горячие-клавиши-hyprland)
- [Конфигурация](#конфигурация)

---

## Возможности

* 🚀 **Два ядра в одной системе**: Автоматическое переключение между `sing-box` (для Hysteria 2, VLESS Reality) и `xray-core` (для XHTTP/SplitHTTP и постквантового шифрования ML-KEM-768).
* 🔒 **Безопасная работа без root в терминале**: Системный демон `omavpnd` управляется через `systemd` с минимальными Linux capabilities (`CAP_NET_ADMIN`, `CAP_NET_BIND_SERVICE`, `CAP_NET_RAW`). Пользователь управляет VPN без ввода пароля `sudo`.
* 🌐 **Прозрачный системный TUN**: Весь трафик приложений, браузеров и терминала направляется через виртуальный интерфейс `omavpn0` без необходимости настраивать системный прокси.
* 📦 **Полная совместимость с подписками Incy / Happ / Hiddify / v2rayNG**: Автоматическая распаковка диплинков (`happ://add/...`), генерация валидных аппаратных заголовков устройства (`x-hwid`, `x-device-os: Linux`) и предотвращение блокировок со стороны провайдеров.
* 📊 **Красивый TUI**: Управление серверами, подписками, просмотр логов и настроек в терминале на базе `ratatui` в стиле темы Omarchy.
* ⚡ **Параллельный пинг**: Асинхронное измерение TCP-задержки по всем сохранённым узлам одной кнопкой (`p`).
* 🖥️ **Waybar & Hyprland Ready**: Команда `omavpn status --waybar` возвращает JSON для трея, а `omavpn toggle` мгновенно переключает VPN хоткеем.

---

## Как устроен и работает omavpn

### 1. Архитектура двойного ядра (Dual-Engine)

Современные VPN-провайдеры (например, Happ, Incy, Marzban) всё чаще переходят на передовые протоколы обхода блокировок:
* **XHTTP (SplitHTTP)** с режимом потоковой выгрузки (`mode: stream-up`).
* **Постквантовое шифрование VLESS ML-KEM-768** (`mlkem768x25519plus...`).

Ядро `sing-box` (версий 1.12–1.14) не поддерживает транспорт `xhttp` и постквантовую криптографию VLESS, из-за чего подключение к таким серверам завершается ошибкой `unknown version: 72` (HTTP 400 Bad Request). В свою очередь, ядро `xray-core` полностью поддерживает XHTTP и ML-KEM, но не имеет встроенного легковесного TUN-роутера уровня `sing-box` с удобным стеком gVisor.

`omavpn` объединяет преимущества обоих ядер:
* **Автовыбор (`CoreEngine::Auto`)**: Клиент анализирует параметры выбранного профиля. Если профиль использует XHTTP или VLESS Encryption — запускается связка с `xray-core`. Для Hysteria 2, VLESS Reality, Trojan, Shadowsocks — используется легковесный `sing-box`.
* **Ручной выбор (`sing-box` / `xray-core`)**: Вы можете принудительно переключить ядро в TUI кнопкой `c` или в настройках.

```
                  ┌───────────────────────────────────────────────┐
                  │                 omavpn (CLI / TUI)            │
                  └───────────────────────┬───────────────────────┘
                                          │ Unix Domain Socket
                                          │ (/run/omavpn/omavpnd.sock)
                  ┌───────────────────────▼───────────────────────┐
                  │              omavpnd (System Daemon)          │
                  │    CAP_NET_ADMIN, CAP_NET_BIND_SERVICE, RAW   │
                  └───────────────────────┬───────────────────────┘
                                          │
                  ┌───────────────────────┴───────────────────────┐
                  ▼                                               ▼
     [Стандартные профили]                           [XHTTP / ML-KEM профили]
     ┌───────────────────────┐                       ┌───────────────────────┐
     │       sing-box        │                       │       sing-box        │
     │      (TUN omavpn0)    │                       │  (TUN omavpn0: gVisor)│
     └───────────┬───────────┘                       └───────────┬───────────┘
                 │                                               │ SOCKS5: 127.0.0.1:10809
                 │                                   ┌───────────▼───────────┐
                 │                                   │       xray-core       │
                 │                                   │ (XHTTP + ML-KEM-768)  │
                 │                                   └───────────┬───────────┘
                 ▼                                               ▼
         Интернет / Сервер                               Интернет / Сервер
```

---

### 2. Гибридный TUN-мост и защита от routing loop

Для профилей, требующих `xray-core`, при включении режима TUN запускается гибридный мост:
1. `xray-core` запускается как локальный апстрим-прокси и слушает loopback-порт `127.0.0.1:10809`.
2. `sing-box` создаёт системный TUN-интерфейс `omavpn0` и объявляет себя шлюзом по умолчанию (`auto_route: true`). Весь трафик системы заворачивается в `omavpn0`, откуда `sing-box` перенаправляет его в локальный SOCKS5 порт Xray.

#### Защита от петли маршрутизации (Loop Prevention):
Когда трафик системы заворачивается в TUN-интерфейс, исходящие пакеты самого процесса `xray-core` к удалённому VPN-серверу рискуют попасть обратно в `omavpn0`, образуя бесконечную петлю. `omavpn` предотвращает это на двух уровнях:
1. **Привязка к физическому интерфейсу (`SO_BINDTODEVICE`)**:
   `omavpn` автоматически сканирует `/proc/net/route`, находит реальный сетевой интерфейс (`eth0`, `wlan0`, `enp3s0`) и конфигурирует сокет Xray через `streamSettings.sockopt.interface`. Пакеты Xray отправляются ядром Linux напрямую в физический интерфейс в обход таблицы маршрутизации TUN.
2. **DNS & CIDR Direct Bypass**:
   Все IP-адреса и подсети VPN-сервера (`87.86.92.0/24`) добавляются в правила `route.rules` sing-box с действием `outbound: direct`. Даже если пакет коснётся sing-box, он мгновенно отправляется напрямую.

---

### 3. Сетевой стек gVisor в TUN

В отличие от стандартного стека `system`, требующего поддержки специфических модулей ядра Linux (iptables/nftables packet mangling, conntrack), `omavpn` использует пользовательский сетевой стек **`gVisor`**, встроенный в `sing-box`.

* **Преимущества gVisor**:
  * Обработка TCP/IP полностью в пространстве пользователя (userspace).
  * Корректная обработка MTU, MSS Clamping и отсутствие потерь фрагментированных TLS ClientHello (размером > 1500 байт).
  * 100% стабильность работы в **WSL2**, Docker-контейнерах и кастомных ядрах Arch Linux.

---

### 4. Разделение привилегий (Daemon + Client)

Безопасность — ключевой приоритет `omavpn`:
* Системный демон **`omavpnd`** запускается как systemd-сервис и обладает правами на создание сетевых интерфейсов и модификацию таблиц маршрутизации через Linux Ambient Capabilities.
* Демон слушает Unix-сокет `/run/omavpn/omavpnd.sock` с правами доступа `0660` и группой `wheel`.
* Пользовательский интерфейс **`omavpn`** (CLI и TUI) запускается от имени вашего обычного пользователя (состоящего в группе `wheel`), общается с демоном через компактный протокол JSON-lines IPC и **никогда не требует `sudo` при повседневном использовании**.

---

### 5. Поддержка подписок Incy / Happ и эмуляция HWID

Многие современные сервисы отклоняют запросы сторонних клиентов, если те не передают специальные заголовки или передают неизвестный User-Agent, отдавая вместо серверов заглушку вида `🚫 Это приложение не поддерживается (0.0.0.0:1)`.

`omavpn` решает эту проблему:
* **Поддержка диплинков**: Распознаёт ссылки вида `happ://add/https://...`, `v2raytun://`, `v2rayng://`, `hiddify://` и автоматически извлекает настоящий HTTP(S) URL подписки.
* **Генерация аппаратного отпечатка (HWID)**: Формирует стабильный хеш оборудования на основе `/etc/machine-id` (или имени хоста), подставляя заголовки:
  * `x-hwid`: `42a9b3f0...`
  * `x-device-os`: `Linux`
  * `x-ver-os`: текущая версия ядра (`6.6.x` / `6.12.x`)
  * `x-device-model`: имя хоста системы
  * `x-device-locale`: `ru_RU` (или текущая системная локаль)
* **Адаптивный User-Agent**: По умолчанию использует актуальный заголовок `Happ/3.9.1 (Linux; x86_64)`, гарантируя получение полного рабочего списка узлов.

---

## Поддерживаемые протоколы

| Протокол | Транспорты | Безопасность / Шифрование | Движок по умолчанию |
| :--- | :--- | :--- | :--- |
| **VLESS** | XHTTP (SplitHTTP) | ML-KEM-768 (Постквантовое), none | **xray-core** (Auto) |
| **VLESS** | gRPC, WS, HTTPUpgrade, TCP | Reality, TLS, none | **sing-box** |
| **Hysteria 2** | UDP (QUIC / BBR) | Salamander Obfuscation, TLS | **sing-box** |
| **Trojan** | TCP, WebSocket, gRPC | TLS, Reality | **sing-box** |
| **Shadowsocks**| TCP, UDP | AEAD (aes-256-gcm, chacha20...) | **sing-box** |
| **VMess** | TCP, WebSocket, gRPC | Auto, AES-128-GCM, Chacha20 | **sing-box** |
| **SOCKS5 / HTTP** | TCP | NoAuth / UserPass | **sing-box** |

---

## Установка

### Способ 1. Arch Linux пакет (.pkg.tar.zst) — *Рекомендуется*

Скачайте релизный пакет из [GitHub Releases](../../releases) и установите через `pacman`:

```bash
# Установка пакета и автоматическое разрешение зависимостей
sudo pacman -U omavpn-0.1.0-1-x86_64.pkg.tar.zst
```

Убедитесь, что демон включён и запущен:
```bash
sudo systemctl enable --now omavpnd
```

---

### Способ 2. Готовый бинарный архив (Tarball)

Скачайте архив `omavpn-v0.1.0-linux-x86_64.tar.gz` из релизов:

```bash
tar -xzvf omavpn-v0.1.0-linux-x86_64.tar.gz
cd omavpn-v0.1.0-linux-x86_64
sudo ./install.sh
```

Скрипт автоматически:
1. Скопирует `omavpn` и `omavpnd` в `/usr/bin/`.
2. Установит и активирует `omavpnd.service`.
3. Проверит наличие `sing-box` и `xray`.

---

### Способ 3. Сборка из исходников

Требуется установленный `rust` и `cargo`:

```bash
# Клонирование репозитория
git clone https://github.com/mrros/omavpn.git
cd omavpn

# Сборка оптимизированных релизных бинарников
cargo build --release

# Установка через скрипт
sudo ./install.sh
```

---

## Использование

### Интерактивный TUI

Запустите TUI без аргументов:
```bash
omavpn
```

```
┌ omavpn 0.1.0 ───────────────────────────────────────── [TUN (System)] [Core: Auto] ┐
│ [1] Servers   [2] Subscriptions   [3] Logs   [4] Settings                           │
├─────────────────────────────────────────────────────────────────────────────────────┤
│  * 1. [VLESS] 🇳🇱 Нидерланды ⭐️             x.nl.vpnpplvpn.top:8008       (47ms)     │
│    2. [Hy2]   🇳🇱 Нидерланды 🎮             87.86.92.19:4443              (27ms)     │
│    3. [VLESS] 🇵🇱 Польша ⚡️                 g.pl.pripripipl.top:8443      (37ms)     │
│    4. [VLESS] 🇫🇮 Финляндия 🏎               node-sh.vpnpplvpn.top:8280    (34ms)     │
│    5. [Hy2]   🇵🇱 Польша 🎮                 g.pl.pripripipl.top:9443      (36ms)     │
├─────────────────────────────────────────────────────────────────────────────────────┤
│ Connected: 🇳🇱 Нидерланды 🎮 (TUN (System)) | 27ms | [Space] Toggle | [q] Quit       │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

#### Горячие клавиши TUI:
* `j` / `k` или `↑` / `↓` — выбор сервера из списка
* `Enter` — подключиться к выбранному серверу
* `Space` — включить / отключить туннель
* `p` — запустить параллельный TCP-пинг всех серверов
* `s` — отсортировать серверы по наименьшей задержке
* `a` — добавить ссылку на подписку или конфигурацию вручную через диалоговое окно
* `i` — импортировать ссылку/подписку из буфера обмена Wayland (`wl-paste`)
* `/` — быстрый поиск/фильтрация по названию или стране
* `c` — переключить ядро (`Auto` $\rightarrow$ `sing-box` $\rightarrow$ `xray-core`)
* `m` — переключить режим работы (`TUN (System)` $\leftrightarrow$ `Proxy-only`)
* `r` — переключить политику маршрутизации (`Bypass LAN` $\leftrightarrow$ `Global`)
* `1`–`4` / `Tab` — переход между вкладками (`Серверы`, `Подписки`, `Логи`, `Настройки`)
* `q` или `Ctrl+C` — выход из интерфейса (туннель в фоне продолжает работать!)

---

### Управление через CLI

```bash
# Подключение по порядковому номеру из списка:
omavpn up 1               # Подключиться к серверу №1
omavpn up 2               # Подключиться к серверу №2

# Подключение по названию или фрагменту имени:
omavpn up "Нидерланды"
omavpn up "Польша"

# Отключение VPN:
omavpn down

# Быстрое переключение (вкл / выкл):
omavpn toggle

# Просмотр текущего статуса:
omavpn status

# Вывод списка всех сохранённых профилей с пингами:
omavpn list

# Пинг всех профилей:
omavpn ping

# Добавление и обновление подписок:
omavpn sub add my-sub "https://provider.com/sub/token"
omavpn sub add happ-link "happ://add/https://provider.com/sub/token"
omavpn sub update         # Обновить все добавленные подписки

# Импорт ссылки вручную или из буфера:
omavpn import "vless://..."
omavpn import             # Импортировать из буфера Wayland

# Просмотр системных логов ядра в реальном времени:
omavpn logs -n 50
```

---

### Интеграция с Waybar

В комплекте поставляется готовый сниппет конфигурации для Waybar (`packaging/waybar.jsonc`).

Добавьте модуль в ваш `~/.config/waybar/config.jsonc`:

```jsonc
"custom/omavpn": {
    "format": "{}",
    "return-type": "json",
    "exec": "omavpn status --waybar",
    "on-click": "omavpn toggle",
    "on-click-right": "foot -T omavpn-float -e omavpn",
    "interval": 2,
    "tooltip": true
}
```

Стили для `~/.config/waybar/style.css`:
```css
#custom-omavpn {
    padding: 0 10px;
    margin: 0 4px;
    border-radius: 6px;
    color: #cdd6f4;
}

#custom-omavpn.connected {
    color: #a6e3a1;
    background-color: rgba(166, 227, 161, 0.15);
    font-weight: bold;
}

#custom-omavpn.disconnected {
    color: #6c7086;
}
```

---

### Горячие клавиши Hyprland

Добавьте следующие строки в `~/.config/hypr/hyprland.conf`:

```ini
# Быстрое переключение VPN хоткеем Super + V
bind = $mainMod, V, exec, omavpn toggle

# Открытие TUI в удобном плавающем окне по Super + Shift + V
bind = $mainMod SHIFT, V, exec, foot -T omavpn-float -e omavpn
windowrulev2 = float, title:^(omavpn-float)$
windowrulev2 = size 900 600, title:^(omavpn-float)$
windowrulev2 = center, title:^(omavpn-float)$
```

---

## Конфигурация

Все пользовательские данные хранятся в формате JSON в директории `~/.config/omavpn/`:

* `profiles.json` — сохранённые VPN-профили и результаты пингов.
* `subscriptions.json` — источники подписок и дата последнего обновления.
* `settings.json` — настройки пользователя:
  ```json
  {
    "mode": "tun",
    "routing": "global",
    "engine": "auto",
    "tun_interface": "omavpn0",
    "tun_mtu": 9000,
    "mixed_port": 10808,
    "ipv6_enabled": false,
    "remote_dns": "https://1.1.1.1/dns-query",
    "local_dns": "local",
    "log_level": "info",
    "selected_profile_id": "...",
    "direct_domains": [],
    "direct_cidrs": []
  }
  ```

---

## Удаление

Если вы устанавливали через pacman:
```bash
sudo pacman -R omavpn
```

Если вы устанавливали через скрипт:
```bash
sudo ./uninstall.sh
```

---

## Лицензия

Проект распространяется под лицензией **GNU General Public License v3.0**. Подробности в файле [LICENSE](LICENSE).
