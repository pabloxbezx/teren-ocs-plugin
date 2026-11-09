# Terén – plugin pro Open CAD Studio

*English: [README.md](README.md)*

Body zaměření, povrchy TIN, spojnice (lomové hrany), hranice, vrstevnice,
prohlížeč 3D a export povrchu do LandXML / DXF – přímo ve výkrese
Open CAD Studia. Rozhraní česky nebo anglicky. Stručný manuál je ve složce
`manual` (PDF česky i anglicky).

## Instalace

Každé vydání Terénu je sestavené pro jednu verzi Open CAD Studia (program
kontroluje, že obě strany používají stejný formát dat výkresu):

| Open CAD Studio | Terén |
|---|---|
| 2026.45 | 1.0.2 – 1.0.4 |
| 2026.44 | 1.0.0 – 1.0.1 |

- **Správce pluginů** v Open CAD Studiu: vyhledejte *Terén* a klikněte
  Instalovat.
- **Ručně (Windows):** stáhněte `Teren-windows.zip` z vydání (Releases),
  rozbalte ho a spusťte `nainstalovat.bat`. Plugin se zkopíruje do
  `%APPDATA%\OpenCADStudio\plugins\cz.fluvio.teren`.
- **Ze zdrojů:** `cargo build --release -p teren_plugin` (libovolná stabilní
  verze Rustu) a knihovnu spolu s `plugin/plugin.toml` zkopírujte do složky
  pluginů. `nainstalovat.bat` bez přiloženého `teren_plugin.dll` udělá
  totéž.

## Ukázkový výkres

`ukazka/teren-sample.dwg` – malé zaměření v lese s povrchem stávajícího
terénu: body se styly (stromy, sloupy, šachty…), spojnice, obrys,
vrstevnice a popisy. Otevřete ho s nainstalovaným Terénem a v Prospektoru
si prohlédněte body, skupiny, povrch a styly.

## Verze 1.0.4

- **Posun bodů úchopem**: tlačítko *Posun bodů* zapne na každém bodě vlastní
  úchop; tažením se bod (i se značkou a popisy) živě posune, jeden krok Zpět.
- **Panel Bod Terénu** v doku (tlačítko *Vlastnosti bodu*): číslo, výška,
  kód a styl vybraných bodů, přímo k úpravě.
- **Kurzorové menu (quad)**: záložka Terén u bodů a povrchů.
- Názvy objektů Terénu ve Vlastnostech i anglicky.
- Obsahuje i změny z 1.0.3 (samostatně nevydaná).

## Verze 1.0.3

- **Posun bodu tažením**: chyťte značku nebo popis bodu a přesuňte ho –
  přesune se celý bod (výška zůstane) a povrch se přepočítá. Volitelně
  (Nastavení) posunutím popisu jen odsunete popis; *Popisy na místo* ho vrátí.
- **Úprava v tabulce bodů**: dvojklik na číslo, kód nebo výšku, Enter uloží
  a skočí na další bod.
- **Hromadně**: vybraným bodům nastavit nebo přičíst výšku, nastavit kód.
- Ve výkresech z dřívějších verzí jednou klikněte na *Překreslit body*
  (Prospektor › Výkres), aby šly posouvat i popisy.

## Verze 1.0.2

- Pro Open CAD Studio **2026.45** (Terén 1.0.1 v něm nepoběží).

## Verze 1.0.1

- Ukázkový výkres, Zoom vše bez počátku souřadnic, tvary značek nezůstávají v počátku.

## Verze 1.0.0

- Pro Open CAD Studio **2026.44** (rozhraní pro pluginy 14). Plugin už
  nemusí být sestavený stejnou verzí Rustu jako Open CAD Studio – vydání
  obsahuje hotové knihovny pro Windows, Linux a macOS a Správce pluginů je
  nainstaluje sám.
- **Angličtina**: Prospektor › Nastavení › Jazyk / Language (automaticky,
  čeština, angličtina); projeví se po novém spuštění Open CAD Studia. Texty
  jsou v `preklady/en.txt`.
- **Ruční bod**: po kliknutí se otevře okno s výškou (návrh z povrchu nebo
  z předchozího bodu) a kódem.
- **Jeden krok Zpět**: import, přepočet, překreslení bodů, styl, šablona…
  se vrací jedním Ctrl+Z (dřív po kouscích).
- **Prohlížeč 3D**: barvu spojnic, obrysu, vrstevnic, bodů a hran
  trojúhelníků zvolíte čtverečkem vedle položky (pamatuje se); obrys a
  spojnice se zapínají zvlášť.
- Manuál PDF (česky a anglicky).

## Historie verzí

### Verze 0.10.5

- **Značka bodu je blok**: každá značka (křížek, strom, šachta…) je jedno
  vložení bloku `TER_ZN_…` – jeden objekt, jeden úchop, menší výkres.
  Stávající výkres převedete tlačítkem *Překreslit body*.
- **Vlastní značka z bloku**: ve stylu bodu zvolte tvar *vlastní – blok
  výkresu* a vyberte blok (vložený nebo vytvořený ve výkrese). Základní bod
  bloku = poloha bodu, blok se zvětší na velikost značky ze stylu; barvu
  dává styl, když má blok barvu „podle bloku“.
- **Bod se vybírá celý** i ve výkrese otevřeném bez spuštění příkazu Terénu
  (dřív se klikem vybrala jen jedna část).
- **Rychlejší instalace**: knihovna DWG (acadrust) se sestavuje s nižší
  optimalizací – sestavení trvá jednotky minut místo desítek a bere méně
  paměti.

### Verze 0.10.4

- **Popisy bodů se nepřekrývají**: popis, který by zakryl jiný popis nebo
  značku, se odsune (vlevo, nahoru, dolů…) a k bodu vede vodicí čára.
  Vypíná se v Nastavení › Výkres a měřítko.
- **Náhled importu**: po výběru souboru Prospektor ukáže první řádky,
  rozpoznaný formát a oddělovač (dají se změnit), prvních 8 bodů a souhrn
  (chybné řádky, body mimo ČR, nesmyslné výšky, opakovaná čísla). Soubor jde
  také přetáhnout do okna. Náhled se dá vypnout.
- **Kontrola zaměření v mapce**: podezřelé body barevně (velikost podle
  odchylky), klik na bod ho ukáže ve výkrese.
- **Šablona výkresu**: Nastavení › Šablona výkresu nebo pás karet
  *Připravit výkres* – hladiny Terénu, značka bodu podle stylu, měřítko a
  styly ze šablony jedním kliknutím; *Uložit tento výkres jako šablonu*.
- Oprava: akce z Prospektoru, které spouštěly příkaz pluginu (měřítko, nový
  povrch ze zaměření, import), nic nedělaly – Open CAD Studio příkazy pluginu
  spuštěné z pluginu nepouští. Teď je provádí přímo Prospektor.

### Verze 0.10.3

- Pro Open CAD Studio **2026.43** (rozhraní pro pluginy verze 13, knihovna
  DWG acadrust 0.6.6). Starší Open CAD Studio 2026.42 tuto verzi nenačte –
  pro něj zůstává Terén 0.10.2.
- **Export povrchu** (detail povrchu v Prospektoru, pravé tlačítko na
  povrchu, pás karet „Export povrchu“):
  - **LandXML** (doporučeno) – výměnný formát povrchů: přesně tytéž
    trojúhelníky, body zaměření a obrys;
  - **DXF – 3D plochy** – každý trojúhelník jako 3DFACE; otevře ho každý CAD.
- **Značku bodu kreslí jen styl**: plugin nastaví ve
  výkrese PDMODE = 1, vlastní značka Open CADu se nekreslí. Styl Standard
  má malé kolečko. Body se symbolem jsou zase na hladině TER_BODY.
- **Bod se vybírá celý**: klik na značku, číslo, výšku nebo popis vybere
  bod i všechny jeho popisy – přesouvají se spolu; výšku a číslo upravíte
  v Prospektoru (Výběr).

### Oprava 0.10.2

- Bod se symbolem (strom, keř, sloup, šachta…) už pod symbolem nekreslí
  ještě svou značku (v 0.10.3 řešeno jinak – viz výše).

### Oprava 0.10.1

- **Ruční bod se ptá na výšku**: klikněte (nebo zadejte x,y), pak napište
  výšku a Enter. Samotný Enter vezme nabízenou výšku (z místa kliknutí,
  z povrchu, nebo jako předchozí bod). S předvolenou výškou se neptá.
- **Odebrat bod** odebere i bod přidaný nástrojem Přidat bod (rozhoduje
  pořadí úprav).
- Ikony stylů v Prospektoru jsou jasné i u nepoužitých stylů a dají se
  kliknout.
- Instalace najde Open CAD Studio i přes zástupce na ploše, kontroluje
  zadanou cestu a nespouští samoaktualizaci rustupu.

### Novinky ve verzi 0.10

- **Skupina bodů v nové podobě**: pravidla jako věty („mají kód T, PAT“,
  „mají číslo 1-50“, „jsou ze souboru …“) s počtem bodů u každého pravidla,
  volba *aspoň jedno pravidlo / všechna pravidla / všechny body*, vyloučení
  (kód, čísla, ručně vytvořené) a **ručně přidané a odebrané body**, které
  mají přednost před pravidly (i „+ z výběru ve výkrese“). Vpravo **náhled**
  bodů obarvený podle toho, proč do skupiny patří; klik na bod to vypíše.
- **Pravé tlačítko v Prospektoru**: nabídky u bodů, skupin, povrchů, stylů
  i u řádků tabulky bodů (ukázat, styl, do skupiny, smazat…). U povrchu:
  aktivní, přepočítat, 3D, styl, krok vrstevnic, přejmenovat, smazat.
- **Průvodce novým povrchem** (Nový povrch › Průvodce…, nebo pravým
  tlačítkem na Povrchy): jméno, skupiny, vyloučené kódy, max. hrana, styl,
  krok vrstevnic a náhled počtu bodů a výšek.
- **Nastavení na jednom místě** (uzel Nastavení ve stromu): měřítko, import,
  nové body, povrchy, kontrola zaměření, chování Prospektoru, obnovení
  výchozích hodnot.
- **Nápověda** (tlačítko Nápověda na pásu karet, v Prospektoru F1 nebo „?“):
  postupy, ovládání a slovníček; kroužek s otazníkem u pojmů ukáže vysvětlivku.
- Srozumitelnější názvy ve stromu: *Z čeho se povrch počítá*, *Spojnice –
  lomové hrany*, *Ruční úpravy trojúhelníků*.
- Hlášky říkají, co udělat (např. proč hranu nejde otočit, kde chybí body).
- Nástroj při prvním spuštění vypíše, co dělá a jak se ovládá (pás karet
  zatím neumí popisky – požadavek je v ZADOST_OCS.md).

### Novinky ve verzi 0.9.1

- **Prohlížeč 3D ovládaný jako SolidWorks**: prostřední tlačítko otáčí kolem
  místa pod kurzorem (žlutý křížek), Ctrl + prostřední / pravé posouvá,
  Shift + prostřední zoomuje, kolečko přibližuje ke kurzoru a oddaluje od
  středu, dvojklik prostředním nebo F = celý povrch, dvojklik = vycentrovat
  místo, Ctrl + dvojklik = ukázat ve výkrese, šipky = otáčení po 15°
  (Shift 90°), Ctrl + šipky = posun, Z / Shift+Z = zoom, Ctrl+1 shora,
  Ctrl+7 izometrie. Pohledy od J/S/V/Z tlačítky i klikem na písmeno
  v kompasu. Svislá osa zůstává svislá.
- Prohlížeč se po změně povrchu **sám načte znovu**.
- **Uložit obrázek** z prohlížeče (PNG do složky výkresu).
- **Rychlejší úpravy povrchu** (otočit / smazat hranu…): přepočet po kliknutí
  potřebuje zhruba o třetinu méně požadavků na Open CAD Studio.
- **Vlastní krok vrstevnic** u povrchu (nabídka Vrstevnice → vlastní…).
- Volba **„Povrch nejde vybrat kliknutím“** (Prospektor → Povrchy): klik
  projde na body a popisy pod povrchem; povrch nejde omylem posunout ani
  smazat, vybírá se z Prospektoru.
- Styly povrchů ve stromu a v přehledu s ikonou a počtem použití.
- Prospektor po uložení stylu neodskočí z editoru stylu.
- Pojistka: když Open CAD Studio neohlásí konec příkazu, automatický
  přepočet a hlídání bodů se po 30 s rozběhnou samy.

### Novinky ve verzi 0.9

- Pro Open CAD Studio **2026.42** (rozhraní pro pluginy verze 10).
- Instalace si verzi Rustu zjistí sama z nainstalovaného Open CAD Studia.
- **Názvy objektů ve Vlastnostech**: „Bod zaměření“, „Značka bodu“, „Číslo
  bodu“, „Výška bodu“, „Popis bodu“, „Povrch TIN“. Body ze starších verzí
  název dostanou po *Překreslit body* (uzel Výkres), povrchy po přepočtu.

### Novinky ve verzi 0.8

- **Prohlížeč povrchu 3D** (Prospektor → záložka *3D*, nebo ve Výběru u povrchu
  *Prohlédnout 3D*): otáčení levým tlačítkem, posun pravým/prostředním, zoom
  kolečkem; převýšení ×1/×2/×5/×10; barvy podle výšky, jednobarevně nebo podle
  sklonu; hrany, body, vrstevnice, obrys a spojnice; výška a souřadnice pod
  kurzorem, dvojklik ukáže místo ve výkrese. Víc povrchů najednou (další
  poloprůhledně). Výkres se nemění.
- **Výběr se drží**: po změně stylu nebo přepočtu z Prospektoru i z pásu karet
  zůstane povrch / bod vybraný ve výkrese i v panelu Výběr.

### Oprava 0.7.1

- Při změně stylu bodu zůstávaly ve výkrese staré značky (hlavně značky ze
  starších verzí pluginu bez odkazu na bod). Nově se smažou všechny značky
  u bodu; značky bez bodu odklidí hlídání bodů, *Překreslit body* (uzel
  Výkres) nebo `TER_AKCE SROVNAT`.

### Novinky ve verzi 0.7

- **Prospektor má dva režimy**: *Výběr* a *Výkres* (přepínač vlevo nahoře).
  - **Výběr** ukazuje vlastnosti toho, na co kliknete ve výkrese. U bodu číslo,
    kód, výška, styl, skupiny a jestli je použit v povrchu; u více bodů společný
    styl a hromadné akce; u povrchu styl, vrstevnice, definice a statistika.
  - **Výkres** je strom celého výkresu jako dosud.
- **Odkud má bod styl**: ručně › podle kódu › podle skupiny › Standard – vidět
  ve Výběru i v tabulce bodů.
- **Aktivní povrch** je vždy vidět nahoře a jde přepnout; zastaralý jde rovnou
  přepočítat.
- Strom: ✔ aktuální / ⚠ zastaralý povrch, skryté povrchy šedě, počet skrytých
  bodů. Styly bodů rozdělené na **Popisy** a **Symboly**, s ikonou a počtem bodů,
  nepoužité šedě.
- Tabulka bodů: filtr podle skupiny a kódu, ikona stylu u každého bodu,
  sloupce jde skrýt.
- Kratší hlášky v příkazovém řádku (import i přepočet na jeden řádek,
  upozornění zvlášť).
- Uzel Výkres: postup **Jak na to** s odškrtáváním hotových kroků.

### Novinky ve verzi 0.6

- **Pás karet uklizen**: Prospektor · Body (Import, Nový bod, **Styl bodu**) ·
  Povrch (Nový povrch, **Styl povrchu**, Najít v Prospektoru) · Definice
  (Spojnice přes body, Přidat spojnici, Hranice, Díra) · Úpravy (6 velkých
  tlačítek) · Analýza (Výška v bodě). Formát a oddělovač importu jsou
  v Prospektoru (Body → Import bodů – formát), přepočet, mazání povrchu,
  vrstevnice a max. hrana u povrchu v Prospektoru, měřítko podle měřítka
  poznámek ve stavovém řádku. Staré příkazy (TER_PREPOCITAT, TER_SMAZAT,
  TER_MERITKO_500…) fungují dál z příkazového řádku.
- **Styl bodu** na pásu karet platí pro **vybrané body** – stačí kliknout na
  značku nebo popis bodu. *Podle skupiny / kódu* vrátí bodům automatický styl.
- **Styl povrchu** na pásu karet platí pro vybrané povrchy (jinak aktivní).
- **Symboly**: strom listnatý, strom jehličnatý, keř, sloup, hydrant, šachta,
  vpusť, šoupě (vestavěné styly i tvary značky ve vlastních stylech).
- **Styl podle kódu**: ve stylu bodů pole *pro kódy* (např. `STR, KER`).
  Pořadí: styl nastavený bodu → styl podle kódu → styl skupiny → Standard.
- Styl **Nezobrazovat** pro body (bod se přesune na vypnutou hladinu
  TER_BODY_SKRYTE, bez značky a popisu) i pro povrchy.
- **Úpravy povrchu se projeví hned** po kliknutí (otočit, přidat a smazat hranu,
  smazat trojúhelník, přidat a odebrat bod) – bez Enter a přepočtu; Enter příkaz
  ukončí.
- **Body hlídají svou grafiku**: po přesunu, smazání nebo kopírování bodu
  se značka a popisy samy srovnají (popisy smazaného bodu zmizí, přesunutý
  nebo zkopírovaný bod dostane popisy na novém místě). Ručně: `TER_AKCE SROVNAT`.
- **Prospektor sleduje výběr** (zaškrtávátko *sledovat výběr*): klik na bod
  nebo povrch ve výkrese ho rovnou ukáže v Prospektoru.
- Skupiny bodů: když je zaškrtnuto *všechny body*, ostatní kritéria se zašedí;
  jinak platí „stačí splnit jedno kritérium“.

### Novinky ve verzi 0.5

- **Styly bodů** (Prospektor → Styly → Styly bodů): značka (jen bod, křížek +/×,
  kolečko, kolečko s křížkem, čtverec, trojúhelník), velikost v mm na papíře,
  popisek (číslo nad výškou, vedle sebe, jen výška, jen číslo, nic), výška písma,
  desetinná místa a čárka, popis (kód), barvy. Náhled přímo v okně.
  Styl se přiřadí **skupině bodů** (pole Styl bodů) nebo **vybraným bodům**
  (tabulka bodů → Styl vybraných; tlačítko *Styl bodů* na pásu karet otevře
  Prospektor na bodech vybraných ve výkrese). Body bez stylu mají styl Standard.
- **Styly povrchů**: vrstevnice (zobrazit, krok vlastní nebo podle povrchu, barva
  a tloušťka vedlejších a hlavních), popis vrstevnic (hlavní / všechny / žádné,
  výška písma, rozestup, desetinná místa, barva), trojúhelníky, obrys,
  **šipky spádu**. Po uložení se povrchy s tímto stylem překreslí.
- Vestavěné styly jde upravit (úprava se uloží do výkresu) a vrátit k původnímu
  nastavení; vlastní styl vznikne tlačítkem *Kopírovat jako nový*.
- **Šablona stylů**: styly výkresu se uloží do profilu uživatele
  (`%APPDATA%\OpenCADStudio\teren_styly.txt`) a načtou do jiného výkresu.
- **Měřítko** se bere z měřítka poznámek vpravo dole ve stavovém řádku Open CAD
  Studia; při jeho změně se značky a popisy bodů i popisy vrstevnic překreslí
  samy. Nabídka Měřítko na pásu karet nastaví totéž.
- Kontrola zaměření: tlačítko *Smazat značky kontroly*.
- Okno Prospektoru si pamatuje polohu a velikost, jde zmenšit na úzký pruh
  (strom se pak přesune nad podrobnosti).

## Záložka Terén (verze 0.4)

**Prospektor** – vlastní okno se stromem výkresu:

- **Body** – tabulka všech bodů: řazení kliknutím na záhlaví, hledání, výběr
  (Ctrl/Shift), dvojklik = ukázat ve výkrese. Úprava čísla, výšky a popisu,
  přečíslování, smazání, nová skupina z vybraných, přidání/odebrání ze skupiny.
  Předvolby nových bodů (kód, výška).
- **Skupiny bodů** – „Všechny body“ a vlastní skupiny podle kódu, čísel a rozsahů
  (`1-50, 101`), původu (soubor importu nebo „(ručně)“), s vyloučením kódů a čísel.
  Každý import vytvoří skupinu „Import <soubor>“. Ze skupiny jde rovnou udělat
  povrch a body exportovat (PYXZD, PNEZD, PENZD…, mezera/čárka/středník/tab,
  Windows-1250 nebo UTF-8).
- **Povrchy** – nastavení, statistika a **Definice**: hranice a díry, spojnice,
  objekty výkresu, úpravy, skupiny bodů. Každou položku jde vypnout (zůstane ve
  výkrese, ale povrch ji nepoužije), ukázat nebo odebrat. Upozornění z přepočtu
  (křížení spojnic, úpravy bez účinku) mají vlastní uzel s tlačítkem Ukázat.
  V uzlu Povrchy jdou vybrané povrchy přepočítat nebo přestylovat najednou.
- **Najít výběr z výkresu** (také tlačítko *Najít v Prospektoru*) – vyberete ve
  výkrese bod, čáru nebo trojúhelník a strom skočí na odpovídající položku.
- **Kontrola zaměření** – podezřelé výšky bodů.

**Body**
- *Import bodů* – formát a oddělovač se rozpoznají automaticky; body dostanou
  skupinu podle souboru.
- *Nový bod* – ruční bod (klik nebo x,y,z; výška zadaná, předvolená, nebo
  z aktivního povrchu) a bod na povrchu. Čísla pokračují od nejvyššího.

**Povrch**
- *Nový povrch*: Stávající terén (skupina „Zaměření“ = všechny body kromě ručně
  vytvořených), Návrh (z vybraných 3D čar), **Z naklikaných bodů** (body se
  uloží do nové skupiny a povrch s automatickým přepočtem vznikne po Enter;
  znovu kliknutý poslední bod = zpět), další povrch.
- *Objekty výkresu* (v Prospektoru u povrchu): body, bloky (výška z vložení),
  čáry a křivky, 3D plochy; volitelně „zachovat hrany“ (čáry jako spojnice).
  Objekty zůstanou na svých hladinách, při jejich změně se povrch přepočítá.

**Definice, Zobrazení, Úpravy, Analýza** – jako ve verzi 0.3 (spojnice přes body,
hranice, díry, styly, vrstevnice, měřítko, otočit/přidat/smazat hranu, výška v bodě).

Nastavení (formát importu, měřítko, vyloučené kódy, práh kontroly, kód a výška
nových bodů) se pamatuje v `%APPDATA%\OpenCADStudio\teren_nastaveni.txt`.

### Jak je to uloženo ve výkrese
- bod: POINT s XDATA `TEREN_BOD` (číslo, popis, soubor importu, vlastní styl);
  značka a popisy bodu jsou na hladinách `TER_BODY_ZNACKA`, `TER_BODY_CISLO`,
  `TER_BODY_VYSKA`, `TER_BODY_POPIS`
- styly: pomocné body na vypnuté hladině `TER_STYLY` (nemazat)
- skupina bodů: pomocný bod na vypnuté hladině `TER_SKUPINY` (nemazat)
- povrch: blok `TER_POVRCH_<ID>`, definice v XDATA `TEREN_POVRCH` na jeho vložení
  (skupiny, vypnuté položky, úpravy, nastavení); hladiny `TER_<ID>_…`
- objekty výkresu v povrchu nesou v XDATA, ke kterým povrchům patří
- výkresy z verze 0.3 se převedou samy (povrch bere skupinu „Všechny body“)

### Příkaz TER_AKCE
Akce Prospektoru jdou spustit i z příkazové řádky (pro skripty), např.
`TER_AKCE PREHLED`, `TER_AKCE SKUPINA id=T;jmeno=Terén;kody=T`,
`TER_AKCE POVRCH_ZE_SKUPINY T Terén`, `TER_AKCE EXPORT * PYXZD C:\body.txt`.

# Výpočetní jádro

Samostatná knihovna v Rustu pro terén: COGO body → TIN povrch → vrstevnice.
Nezávisí na Open CAD Studiu; plugin je jen tenká vrstva nad ní.
Pro kontrolu výsledků je přiložen program `teren`, který zapisuje DXF.

## Formát bodů

Formát se zadává písmeny v pořadí sloupců (výchozí `PNEZD`, oddělovač mezera):

| písmeno | sloupec |
|---|---|
| P | číslo bodu |
| N, E | souřadnice výkresu (záporné S-JTSK) |
| Y, X | S-JTSK kladné od geodeta – převede se na E = −Y, N = −X |
| Z | výška |
| D | popis; kód = první slovo popisu |
| - | sloupec se přeskočí |

Příklady: `PNEZD`, `PENZD`, `PYXZD` (geodet), `ENZ`, `P-NEZD`.
Oddělovače: `mezera`, `carka`, `strednik`, `tab`. Desetinná čárka se přijme,
pokud čárka není oddělovač. Soubor může být v UTF-8 i Windows-1250.
Řádky začínající `#` se ignorují.

## Program pro příkazový řádek

```
cargo build --release
target/release/teren zamereni.txt --vyloucit STR --max-hrana 40 --interval 0.5 --hlavni 2.5
```

Další volby: `--format`, `--oddelovac`, `--hlavicka N`, `--kody T,DNO`,
`--vyska-textu 0.5`, `--bez-popisu`, `--bez-tin`, `-v vystup.dxf`
(`teren --help` vypíše vše).

Hladiny v DXF: `TER_BODY`, `TER_BODY_CISLO`, `TER_BODY_VYSKA`, `TER_BODY_POPIS`,
`TER_TIN`, `TER_VRSTEVNICE_HLAVNI`, `TER_VRSTEVNICE_VEDLEJSI`.

## Licence

GPL-3.0-only. Autor: Ing. Pavel Janouš.
