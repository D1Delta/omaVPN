# omaVPN 🛡️

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Platform: Arch Linux / Omarchy](https://img.shields.io/badge/Platform-Arch%20Linux%20%7C%20Omarchy-1793D1.svg?logo=arch-linux&logoColor=white)](https://archlinux.org)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-DEA584.svg?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Engines: sing--box + xray--core](https://img.shields.io/badge/Engines-sing--box%20%2B%20xray--core-green.svg)](#как-устроен-и-работает-omavpn)

---

## Общее описание проекта

**omaVPN** — это нативный, быстрый и легковесный консольный/TUI VPN-клиент на языке **Rust**, созданный специально для **Arch Linux** и **Omarchy** (Hyprland + Waybar + systemd).

Проект решает главные проблемы современных Linux-пользователей при работе с прокси-подписками (включая **Incy**, **Happ**, **Hiddify**, **v2rayNG**):
* **Несовместимость новых протоколов с одним ядром**: вместо выбора между `sing-box` и `xray-core` клиент объединяет **оба ядра** и автоматически подбирает нужное под каждый сервер (например, `xray-core` для XHTTP и постквантового шифрования VLESS ML-KEM-768, и `sing-box` для Hysteria 2 и VLESS Reality).
* **Прозрачный системный TUN без `sudo` при каждом запуске**: сетевой туннель (`omavpn0`) поднимает фоновый `systemd`-демон `omavpnd` с минимальными Linux capabilities, а вы управляете подключениями от обычного пользователя через TUI, CLI или иконку в Waybar.
* **Полная поддержка подписок с проверкой устройства**: клиент умеет распаковывать диплинки (`happ://add/...`, `v2raytun://`, `hiddify://`) и передаёт корректные Linux HWID-заголовки, чтобы провайдер отдавал реальные серверы, а не заглушку.
* **Нативная эстетика терминала**: интерфейс на `ratatui` без тяжёлого Electron/Chromium, с быстрой клавиатурной навигацией, параллельным TCP-пингом всех серверов и мгновенным переключением по горячим клавишам Hyprland.

---

## Оглавление
- [Общее описание проекта](#общее-описание-проекта)
- [Инструкция по установке](#инструкция-по-установке)
  - [Шаг 1. Установка зависимостей](#шаг-1-установка-зависимостей)
  - [Шаг 2. Установка omaVPN](#шаг-2-установка-omavpn)
  - [Шаг 3. Запуск системного демона](#шаг-3-запуск-системного-демона)
  - [Шаг 4. Первое подключение](#шаг-4-первое-подключение)
- [Как устроен и работает omaVPN](#как-устроен-и-работает-omavpn)
  - [1. Архитектура двойного ядра (Dual-Engine)](#1-архитектура-двойного-ядра-dual-engine)
  - [2. Гибридный TUN-мост и защита от routing loop](#2-гибридный-tun-мост-и-защита-от-routing-loop)
  - [3. Сетевой стек gVisor в TUN](#3-сетевой-стек-gvisor-в-tun)
  - [4. Разделение привилегий (Daemon + Client)](#4-разделение-привилегий-daemon--client)
  - [5. Поддержка подписок Incy / Happ и HWID](#5-поддержка-подписок-incy--happ-и-hwid)
- [Поддерживаемые протоколы](#поддерживаемые-протоколы)
- [Использование (TUI и CLI)](#использование-tui-и-cli)
- [Интеграция с Waybar и Hyprland](#интеграция-с-waybar-и-hyprland)
- [Конфигурация и удаление](#конфигурация-и-удаление)

---

## Инструкция по установке

### Шаг 1. Установка зависимостей

Для работы клиента требуется `sing-box` (версии **1.14+**), `wl-clipboard` (для импорта ссылок из буфера Wayland) и `xray-core` (для серверов с XHTTP / VLESS ML-KEM-768):

```bash
# 1. Основные пакеты из официальных репозиториев Arch Linux
sudo pacman -S --needed sing-box wl-clipboard

# 2. Установка xray-core (через AUR)
yay -S --needed xray-bin
```

<details>
<summary>Альтернативная установка xray-core напрямую с GitHub (если нет AUR-помощника)</summary>

```bash
curl -L -o /tmp/xray.zip https://github.com/XTLS/Xray-core/releases/latest/download/Xray-linux-64.zip
unzip -o /tmp/xray.zip -d /tmp/xray
sudo install -Dm755 /tmp/xray/xray /usr/bin/xray
sudo mkdir -p /usr/share/xray
sudo cp -f /tmp/xray/*.dat /usr/share/xray/
rm -rf /tmp/xray /tmp/xray.zip
```
</details>

---

### Шаг 2. Установка omaVPN

Выберите любой удобный способ установки:

#### Вариант A. Готовый пакет для Arch Linux / Omarchy *(Рекомендуется)*

Скачайте `omavpn-0.1.0-1-x86_64.pkg.tar.zst` со страницы [GitHub Releases](https://github.com/D1Delta/omaVPN/releases) и установите через `pacman`:

```bash
sudo pacman -U omavpn-0.1.0-1-x86_64.pkg.tar.zst
```

#### Вариант B. Портативный бинарный архив (`.tar.gz`)

Скачайте `omavpn-v0.1.0-linux-x86_64.tar.gz` со страницы [GitHub Releases](https://github.com/D1Delta/omaVPN/releases), распакуйте и запустите скрипт установки:

```bash
tar -xzvf omavpn-v0.1.0-linux-x86_64.tar.gz
cd omavpn-v0.1.0-linux-x86_64
sudo ./install.sh
```

#### Вариант C. Сборка из исходников

Требуется установленный компилятор Rust (`sudo pacman -S rust`):

```bash
git clone https://github.com/D1Delta/omaVPN.git
cd omaVPN
cargo build --release
sudo ./install.sh
```

---

### Шаг 3. Запуск системного демона

Включите автозапуск и стартуйте фоновый сервис `omavpnd`:

```bash
sudo systemctl enable --now omavpnd
sudo systemctl status omavpnd
```

> **Примечание:** Для управления `omavpn` без `sudo` ваш пользователь должен состоять в группе `wheel` (в Arch / Omarchy это настроено по умолчанию). Проверить можно командой `groups`.

---

### Шаг 4. Первое подключение

1. Добавьте ссылку на подписку (поддерживаются `https://...`, `happ://add/...`, `v2raytun://...`, `hiddify://...`) или одиночный ключ:
   ```bash
   omavpn sub add main "https://example.com/your-subscription-link"
   ```
   *(Либо просто запустите `omavpn` и нажмите `a` для ручного ввода или `i` для вставки из буфера обмена).*
2. Проверьте задержку до всех серверов:
   ```bash
   omavpn ping
   ```
3. Подключитесь к первому серверу или запустите интерактивный TUI:
   ```bash
   omavpn up 1     # подключение из консоли
   omavpn          # запуск интерактивного TUI
   ```
4. Убедитесь, что трафик идёт через VPN:
   ```bash
   curl -s https://api.ipify.org
   ```

---

## Как устроен и работает omaVPN

### 1. Архитектура двойного ядра (Dual-Engine)

Современные VPN-провайдеры (Happ, Incy, Marzban) всё чаще используют:
* **XHTTP (SplitHTTP)** с режимом потоковой выгрузки (`mode: stream-up`);
* **Постквантовое шифрование VLESS ML-KEM-768** (`mlkem768x25519plus...`).

`sing-box` 1.14 не поддерживает транспорт `xhttp` и VLESS Encryption, а `xray-core` не обладает встроенным лёгким TUN-маршрутизатором со стеком gVisor. **omaVPN** объединяет оба ядра:
* **`CoreEngine::Auto`** *(по умолчанию)* — автоматически определяет, требует ли выбранный профиль `xray-core` (`xhttp` / VLESS Encryption). Если да — запускается гибридная связка `xray-core` + `sing-box TUN`; если нет (Hysteria 2, VLESS Reality, Trojan, Shadowsocks) — работает один `sing-box`.
* **Ручной выбор** — ядро можно принудительно переключить в TUI клавишей `c`.

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
     │  (TUN omavpn0: gVisor)│                       │  (TUN omavpn0: gVisor)│
     └───────────┬───────────┘                       └───────────┬───────────┘
                 │                                               │ SOCKS5: 127.0.0.1:10809
                 │                                   ┌───────────▼───────────┐
                 │                                   │       xray-core       │
                 │                                   │ (XHTTP + ML-KEM-768)  │
                 │                                   └───────────┬───────────┘
                 ▼                                               ▼
         Интернет / Сервер                               Интернет / Сервер
```

### 2. Гибридный TUN-мост и защита от routing loop

В гибридном режиме `xray-core` поднимает локальный SOCKS5-сервер на `127.0.0.1:10809`, а `sing-box` создаёт системный интерфейс `omavpn0` и направляет туда трафик приложений. Чтобы исходящие пакеты самого Xray не зацикливались обратно в TUN:
1. **Привязка к физическому интерфейсу (`SO_BINDTODEVICE`)** — клиент считывает `/proc/net/route`, определяет активный физический интерфейс (`eth0` / `wlan0`) и передаёт его в `streamSettings.sockopt.interface` для Xray.
2. **Прямой байпас по DNS и CIDR** — домен VPN-сервера и все его разрешённые IP-адреса (включая подсеть `/24`) добавляются в правила `sing-box` с `outbound: direct`.

### 3. Сетевой стек gVisor в TUN

Для интерфейса `omavpn0` используется пользовательский TCP/IP стек **`gVisor`**, встроенный в `sing-box`. Это исключает зависимость от модулей `iptables`/`nftables` ядра, предотвращает проблемы с фрагментацией крупных TLS ClientHello (> 1500 байт) и гарантирует стабильную работу как на «голом железе» Arch Linux, так и внутри **WSL2**.

### 4. Разделение привилегий (Daemon + Client)

* **`omavpnd`** работает под `systemd` с ограниченным набором Linux capabilities (`CAP_NET_ADMIN`, `CAP_NET_BIND_SERVICE`, `CAP_NET_RAW`) и слушает Unix-сокет `/run/omavpn/omavpnd.sock` (`0660`, группа `wheel`).
* **`omavpn`** запускается от обычного пользователя и общается с демоном по JSON-lines IPC. Вводить пароль `sudo` при переключении серверов не нужно.

### 5. Поддержка подписок Incy / Happ и HWID

* Автоматическая распаковка диплинков `happ://add/...`, `v2raytun://`, `v2rayng://`, `hiddify://`.
* Генерация стабильного аппаратного идентификатора на основе `/etc/machine-id` и передача заголовков `x-hwid`, `x-device-os: Linux`, `x-ver-os`, `x-device-model`, `x-device-locale`.

---

## Поддерживаемые протоколы

| Протокол | Транспорты | Безопасность / Шифрование | Движок по умолчанию |
| :--- | :--- | :--- | :--- |
| **VLESS** | XHTTP (SplitHTTP) | ML-KEM-768 (постквантовое), none | **xray-core** (Auto) |
| **VLESS** | gRPC, WS, HTTPUpgrade, TCP | Reality, TLS, none | **sing-box** |
| **Hysteria 2** | UDP (QUIC / BBR) | Salamander Obfuscation, TLS | **sing-box** |
| **Trojan** | TCP, WebSocket, gRPC | TLS, Reality | **sing-box** |
| **Shadowsocks** | TCP, UDP | AEAD (`aes-256-gcm`, `chacha20-ietf-poly1305`) | **sing-box** |
| **VMess** | TCP, WebSocket, gRPC | Auto, AES-128-GCM, Chacha20 | **sing-box** |
| **SOCKS5 / HTTP** | TCP | NoAuth / UserPass | **sing-box** |

---

## Использование (TUI и CLI)

### Интерактивный TUI

```bash
omavpn
```

**Основные клавиши:**
* `j` / `k` или `↑` / `↓` — навигация по списку серверов
* `Enter` — подключиться к выбранному серверу
* `Space` — включить / отключить VPN
* `p` — параллельный TCP-пинг всех серверов
* `s` — сортировка по минимальной задержке
* `a` — добавить подписку или ключ вручную
* `i` — импортировать ссылку из буфера обмена Wayland (`wl-paste`)
* `/` — фильтр по названию, `Esc` — сброс фильтра
* `c` — переключить ядро (`Auto` → `sing-box` → `xray-core`)
* `m` — переключить режим (`TUN (System)` ↔ `Proxy-only`)
* `r` — переключить маршрутизацию (`Bypass LAN` ↔ `Global`)
* `1`–`4` / `Tab` — вкладки (`Servers`, `Subscriptions`, `Logs`, `Settings`)
* `q` — выйти из TUI (туннель продолжит работать в фоне)

### Команды CLI

```bash
omavpn up 1                     # Подключиться к серверу №1 из списка
omavpn up "Нидерланды"          # Подключиться по части названия
omavpn down                     # Отключить VPN
omavpn toggle                   # Включить / выключить VPN
omavpn status                   # Статус подключения
omavpn status --waybar          # JSON-статус для Waybar
omavpn list                     # Список всех профилей
omavpn ping                     # Измерить задержку до всех узлов
omavpn sub add main "<url>"     # Добавить подписку
omavpn sub update               # Обновить все подписки
omavpn import "<uri>"           # Импортировать одиночную ссылку
omavpn logs -n 50               # Показать последние 50 строк логов ядер
```

---

## Интеграция с Waybar и Hyprland

### Waybar (`~/.config/waybar/config.jsonc`)

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

### Hyprland (`~/.config/hypr/hyprland.conf`)

```ini
# Переключение VPN по Super + V
bind = $mainMod, V, exec, omavpn toggle

# Плавающее окно TUI по Super + Shift + V
bind = $mainMod SHIFT, V, exec, foot -T omavpn-float -e omavpn
windowrulev2 = float, title:^(omavpn-float)$
windowrulev2 = size 900 600, title:^(omavpn-float)$
windowrulev2 = center, title:^(omavpn-float)$
```

---

## Конфигурация и удаление

Пользовательские файлы сохраняются в `~/.config/omavpn/`:
* `profiles.json` — сохранённые серверы и результаты пингов;
* `subscriptions.json` — список подписок;
* `settings.json` — режим работы, выбранное ядро, MTU, DNS и исключения.

**Удаление:**
```bash
# Если устанавливали через pacman:
sudo pacman -R omavpn

# Если устанавливали через install.sh:
sudo ./uninstall.sh
```

## Лицензия

Проект распространяется под лицензией **GNU General Public License v3.0**. Подробности — в файле [LICENSE](LICENSE).
