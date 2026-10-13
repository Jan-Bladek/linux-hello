# Instalacja Linux Hello na Fedorze

Zalecana metoda to instalator ze źródeł. Przygotowuje pełne GUI, rozpoznawanie
YuNet/SFace, helpery sudo/KDE, moduł PAM i reguły SELinux. Stary spec RPM służy
wyłącznie do diagnostyki; nie jest paczką pełnej aplikacji. Paczka DEB nie jest
odpowiednia dla docelowej Fedory. Gotowe binarne RPM i podpisane repozytorium
pakietów pozostają osobnym etapem wymagającym testów cyklu instalacji/usuwania.

## Wymagania

- Zwykła, modyfikowalna Fedora 44 lub nowsza, najlepiej KDE Plasma. Testowano Fedorę 44.
- Konto użytkownika z możliwością autoryzacji sudo oraz Polkit w sesji graficznej.
- Internet przy pierwszej instalacji: pakiety Fedory, Rust, zależności Cargo i modele.
- Wolne miejsce na kompilację i czas na zbudowanie GUI/OpenCV; kompilator Rust 1.99.0.
- Zgodna kamera. Automatyczne sterowanie emiterem IR dotyczy wyłącznie
  [sprawdzonego sprzętu](hardware-support.md), nie każdej kamery reklamowanej jako IR.

Fedora Atomic/ostree i dystrybucje Debian/Ubuntu nie są obsługiwane przez ten
instalator. Logowanie i blokada ekranu dotyczą KDE; na innych desktopach wybierz
w GUI tylko obsługiwane zastosowania. PAD i reprezentatywne NIR FAR/FRR nie są
zakwalifikowane; hasło pozostaje dostępne.

## Jedno polecenie po pobraniu repozytorium

Jeśli nie masz jeszcze kodu:

```sh
sudo dnf install git python3
git clone https://github.com/Jan-Bladek/linux-hello.git
cd linux-hello
./install.sh
```

**Uruchom instalator bez sudo.** Sam poprosi o autoryzację do instalacji pakietów
Fedory i plików systemowych. Rust, Cargo, pobieranie modeli, testy i kompilacja
wykonują się jako zwykły użytkownik. Nie kompiluj projektu jako root.

Instalator:

1. Sprawdza Fedorę i instaluje biblioteki kompilacyjne/runtime przez DNF.
2. Używa istniejącego rustup lub pobiera oficjalny instalator Rust do prywatnego
   katalogu użytkownika. Wybiera przypiętą wersję 1.99.0, bez zmiany globalnego
   domyślnego toolchainu. [Oficjalna instalacja Rust](https://doc.rust-lang.org/stable/book/ch01-01-installation.html).
3. Uruchamia testy GUI, helpera i PAM oraz sprawdza dokumentację. Buduje wszystkie
   potrzebne binaria z `Cargo.lock`, z pełnymi feature gates dla rozpoznawania i KDE.
4. Sprawdza rozmiary i SHA256 dwóch przypiętych modeli. Używa poprawnego lokalnego
   cache lub jawnie pobiera modele wraz z informacjami licencyjnymi/proweniencją.
   Modele nie są pobierane przez aplikację podczas logowania.
5. Kompiluje probe PAM i minimalną politykę MMAP kamery. Sprawdza staging i hashe
   przed instalacją. Wymienia pliki atomowo i przywraca poprzednie przy zwykłym
   błędzie. Zachowuje już zainstalowane polityki; nie nadpisuje nieznanych
   ustawień SELinux ani nie przełącza systemu w Permissive.
6. Tworzy skrót **Linux Hello** w menu oraz polecenia `linux-hello` i `hello`.

Instalacja nie zapisuje twarzy, nie zmienia `/etc/pam.d`, nie włącza żadnego konta
ani demona. Jeśli integracje były już aktywne, aktualizacja zachowuje ich wzorce,
klucze, progi i ustawienia. Nie usuwa danych innych użytkowników.

## Pierwsze uruchomienie i aktywacja

Uruchom **Linux Hello** z menu KDE albo:

```sh
linux-hello
```

Zeskanuj twarz, nadaj jej nazwę, wybierz karty **Sudo**, **Logowanie KDE** i/lub
**Odblokowanie**, a następnie **Zapisz i skonfiguruj**. Wybierz osobną frazę
szyfrowania wzorca: minimum 12 znaków, wpisaną dwa razy. Dla już zapisanego wzorca
użyj jego dotychczasowej frazy. W osobnym systemowym oknie administratora używa
się hasła konta Linux — nie frazy wzorca.

Konfiguracja wykonuje rzeczywiste skanowania i testy przed zmianą PAM. Zamknij
podgląd kamery, patrz prosto i czekaj na wynik. Po błędzie poprzednia konfiguracja
jest przywracana. Pełny przepływ opisano w [integracji KDE](kde-login.md#unified-gui-setup).

Przy odświeżaniu dotychczasowej instalacji nie musisz ponownie zapisywać twarzy.
Zamknij stare okno aplikacji i uruchom nowe z menu. Instalator nie wylogowuje,
nie blokuje sesji ani nie restartuje display managera.

## Opcje

| Polecenie | Działanie |
| --- | --- |
| `./install.sh --dry-run` | Pokazuje plan; bez sieci, kompilacji i zmian |
| `./install.sh --build-only` | Zależności, testy, kompilacja i sprawdzenie stagingu; bez instalacji plików aplikacji |
| `./install.sh --skip-deps` | Pomija DNF; wymagane biblioteki muszą już być obecne |
| `./install.sh --skip-deps --offline` | Bez sieci; wymaga toolchainu, cache Cargo, bibliotek i poprawnych modeli |
| `./install.sh --with-tpm` | Instaluje także opcjonalną politykę dostępu TPM dla logowania KDE |

`--build-only` sam w sobie może instalować zależności przez DNF; dodaj
`--skip-deps`, jeśli nie chcesz tego kroku. Przykład kontroli istniejącej maszyny:

```sh
./install.sh --build-only --skip-deps --offline --with-tpm
```

`--with-tpm` nie migruje kluczy i nie resetuje TPM. Po poprawnym zapisaniu
wzorca i aktywacji wybranych usług możesz świadomie migrować klucz:

```sh
sudo /usr/local/libexec/linux-hello-sudo/hello-sudo tpm-enable "$(id -u)"
sudo /usr/local/libexec/linux-hello-sudo/hello-sudo tpm-status "$(id -u)"
```

Migracja nie wymaga nowego skanowania. Najpierw przeczytaj [zasady i ograniczenia TPM](tpm.md).
Już zmigrowany profil pozostaje zabezpieczony także przy aktualizacji bez `--with-tpm`.
Instalator zachowuje istniejącą politykę TPM. Nie ma w tym trybie atestacji rozruchu.

## Pliki, aktualizacja i powrót

Binaria, worker, modele i skrypty znajdują się w
`/usr/local/libexec/linux-hello-sudo/`; PAM w `/usr/lib64/security/pam_linuxhello.so`.
Komendy są w `/usr/local/bin/`, skrót systemowy w `/usr/local/share/applications/`
i skrót bieżącego użytkownika w `~/.local/share/applications/`. Poprzedni skrót
użytkownika jest zachowany jako `linux-hello.desktop.before-install`.

Manifest root-only `install-manifest.json` zapisuje rewizję źródeł i hashe plików.
Dopisane `-dirty` oznacza kompilację z lokalnymi zmianami. Profile biometryczne
pozostają oddzielnie w `/var/lib/linux-hello/sudo/`, a szyfrowany wzorzec GUI w
katalogu danych użytkownika. Staging jest prywatny i po zakończeniu usuwany.
Źródła, zależności Cargo i pobrane modele pozostają w cache użytkownika.

Aktualizacja:

```sh
git pull --ff-only
./install.sh
```

W razie zwykłego błędu instalator przywraca poprzednie pliki, usuwa dodane przez
niego reguły kontekstu/moduły i zachowuje wcześniejsze. Nie gwarantuje transakcji
wielu plików przy odcięciu zasilania lub SIGKILL. Mogą pozostać puste katalogi.

Nie usuwaj helpera ani modułu PAM, gdy logowanie twarzą jest aktywne. Najpierw
wyłącz integracje według [sudo](sudo-integration.md) i [KDE](kde-login.md), dla
wszystkich kont, które je wykorzystują. Ten instalator nie usuwa automatycznie
wzorców, kluczy, konfiguracji innych administratorów ani reguł używanych przez
inne instalacje. Nie jest zarządzany przez RPM, więc `dnf remove linux-hello`
nie usuwa tych ręcznie zainstalowanych plików.

## Zakres testów

Przeszedł pełny build i staging z lokalnymi zależnościami/modelami na Fedorze 44.
Testy instalatora obejmują nieobsługiwany system, zmienione pliki, złe modele,
dowiązania, hardlinki, niedozwolone ścieżki i przywracanie poprzedniego PAM/helpera.
Izolowany test sprawdził pusty zestaw plików instalacyjnych, uprawnienia i CLI,
z istniejącymi politykami SELinux, bez rzeczywistych wzorców i aktywacji PAM.
To nie zastępuje testu pierwszej instalacji na czystej VM ani kwalifikacji kamery.
Szczegółowe wykonane wyniki są w [validation](validation.md).
