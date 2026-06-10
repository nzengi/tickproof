# tickproof — Vizyon, Geliştirme Alanları ve Roadmap

> Çalışma dokümanı. Karar noktaları `[KARAR]`, gelir alanları `[GELİR]`,
> araştırma gerektirenler `[ARŞ]` ile işaretli. Her iş paketi tek başına
> bir geliştirme oturumuna verilebilecek granülaritede yazıldı.

---

## 0. Tez ve Konumlandırma

### 0.1 Keşif ne?

Tek cümle: **Solana, kendi VM'inde yazılmış herhangi bir deterministik
hesaplamayı, off-chain native hızda çalıştırıp ~19k CU'luk bir fraud
proof ile hesap verebilir kılabilir.** Rollup'lar bunu yapamaz çünkü
L1'leri kendi VM'lerini çalıştıramaz — interpreter ya da zk devresi
yazmak zorundalar. Solana'da SBF derlemesi *hem* off-chain engine'de
*hem* zincirde aynı bytecode olarak koşar. Fraud proof = sıradan bir CPI.

Ölçülmüş temel (devnet, gerçek transaction'lar):

| metrik | değer |
|---|---|
| bir arena tick'i | ~2,000 CU |
| tam one-step fraud proof | ~19k CU (tx bütçesinin %1.4'ü) |
| trustless settle (root + verdict CPI + ödeme) | ~13k CU |
| karşılaştırma: in-contract interpreter | ~157k CU/tick (proof'suz) |
| karşılaştırma: SP1 Groth16 verify | ~280k CU |
| engine throughput | ~17k tick/s/çekirdek |
| dürüst maç (devnet) | 6 tx / ~31 s |
| adversarial maç (devnet) | 20 tx / ~47 s |

### 0.2 MagicBlock'a karşı konum

MagicBlock (ephemeral rollups): hesabı geçici bir yüksek-hızlı SVM
instance'ına delege eder, orada hızlı çalıştırır, sonucu ana zincire
commit eder. **Güven modeli**: ephemeral validator'a (TEE/operatör
setine) güvenirsin; itiraz mekanizması hesabın geri çekilmesidir, tek
tick'lik bir cinayet kanıtı değildir.

tickproof'un farkı:

- **Güven değil, kanıt.** Operatör ne kadar kötü niyetli olursa olsun,
  tek bir tick'lik yalan zincirin kendisi tarafından native replay ile
  çürütülür. TEE yok, committee yok, "trust me" yok.
- **Altyapı maliyeti sıfıra yakın.** Ephemeral validator kümesi işletmek
  gerekmiyor; engine herhangi bir makinede koşar, zincir sadece itiraz
  anında devreye girer.
- **Tamamlayıcı olabilir.** [KARAR] MagicBlock'un hız katmanının üstüne
  tickproof'un verifiability katmanı eklenebilir (ephemeral session'ın
  sonucunu tickproof checkpoint'i olarak commit etmek). Rakip değil
  katman olarak konumlanmak ekosistem politikası açısından daha akıllıca
  olabilir.

Zayıf olduğumuz yer (dürüst olalım): MagicBlock *genel* Solana
programlarını hızlandırır; tickproof `TickLogic` disiplinine uyan
(no_std, float'sız, heap'siz, saf state geçişi) programlar ister. Bu
bir kısıt ama aynı zamanda determinizm garantisinin kaynağı.

### 0.3 Moat (savunulabilirlik)

1. Çalışan, ölçülmüş, uçtan uca pipeline (çoğu rakip fikir aşamasında).
2. Determinizm disiplini ve test altyapısı (golden vectors, bit-exact
   SBF/native eşleşmesi) — kopyalaması kolay görünür, doğru yapması zor.
3. Verdict CPI standardı tutarsa ağ etkisi: her oyun aynı escrow'u,
   aynı turnuva programını, aynı watchtower'ı kullanır.

---

## 1. Geliştirme Ağacı — Çekirdek Protokol

### 1.1 Input Authenticity ★ (en kritik açık)

**Sorun:** Input chain *hangi* input'ların commit edildiğini sabitler,
*kimin* ürettiğini değil. Session operatörü rakibin input'larını uydurup
"rakip hiç hamle yapmadı" diyebilir.

Çözüm yolları [KARAR — ikisi birlikte de olabilir]:

- **A) İmzalı input entry'leri.** Her input entry'ye oyuncu imzası;
  tick fonksiyonu içinde doğrulama. Ed25519 in-SBF pahalı → Solana'nın
  ed25519 precompile'ı ile tx-level doğrulama + replay instruction'ında
  precompile introspection. [ARŞ] Replay tx'inde ed25519 program
  instruction'ı introspect etme deseni (sysvar instructions).
  - İş paketi 1: input entry formatına `player_sig` alanı + tick-core'da
    doğrulama trait'i (`InputAuth`), native testler.
  - İş paketi 2: replay instruction'ına ed25519 introspection.
  - İş paketi 3: engine'de imza üretimi/doğrulaması, devnet-match'e
    entegrasyon.
- **B) On-chain input posting (turn-based için).** Yavaş oyunlarda
  input'lar doğrudan zincire yazılır; chain == authenticity. Hızlı
  oyunlarda değil ama poker/satranç sınıfı için bedava çözüm.
- **C) Karşılıklı imzalı checkpoint'ler.** Her N tick'te iki oyuncu da
  checkpoint'i co-sign eder; itiraz penceresi sadece son co-sign'dan
  sonrası için açık kalır. Dispute yüzeyini küçültür.

### 1.2 Data Availability (input log DA)

**Sorun:** Challenger bisection oynayabilmek için input log'una muhtaç.
Operatör log'u saklarsa (withholding) challenger mid-claim'leri
hesaplayamaz.

- Mevcut hafifletme: timeout asimetrisi (cevap vermeyen kaybeder) +
  iki oyuncunun da kendi log kopyası olması (P2P maçta ikisi de input
  akışını canlı görür).
- [ARŞ] Genel çözüm: DA challenge oyunu ("şu tick aralığının log'unu
  zincire yaz, yoksa kaybedersin"), log chunk'larını input chain'e
  bağlayan merkle yapı.
- [KARAR] İlk ürünlerde "iki taraf da log'u canlı tutar" varsayımı
  yeterli; DA oyununu platform fazına ertele.

### 1.3 Referee v2

- **Multi-challenger:** şu an session başına tek challenger; resolved
  session ölü. Tasarım: assertion başına challenge kuyruğu ya da
  challenge'ı ayrı PDA'ya taşıyıp session'ı yaşatmak.
- **Çoklu eşzamanlı assertion** (pipelined checkpoints): şu an tek
  in-flight assertion var; throughput için pencere zinciri.
- **Bond eğrisi:** bisection turu başına artan bond (griefing'i
  pahalılaştırır), kazanana gas iadesi.
- **PDA'lı session'lar:** şu an throwaway keypair; PDA (match, player,
  nonce) türetimi ile cüzdansız keşfedilebilirlik.
- **Mainnet parametreleri:** pencere süreleri (saatler), bond
  büyüklükleri, rent stratejisi; ekonomik saldırı analizi dokümanı.

### 1.4 State ölçekleme

- **Partial-state replay** [ARŞ]: 10KB+ state'lerde tüm pre-state'i tx'e
  koymak yerine, tick'in dokunduğu chunk'lar + merkle proof'larıyla
  replay. Merkle kütüphanesinde `prove/verify` zaten var; eksik olan
  tick'in "okuduğu/yazdığı chunk seti"ni deklare etmesi (access list).
  Bu, tickproof'u "küçük oyun state'i" kısıtından kurtarır → mikro-RTS,
  daha zengin simülasyonlar.
- **Tick batching:** dispute'ta tek tick yerine k-tick replay (CU
  bütçesine sığdığı sürece) → bisection turu log₂(n/k)'ya düşer, devnet
  ölçümüyle optimal k seçimi.
- **State sıkıştırma:** checkpoint'ler arası delta-log.

### 1.5 Güvenlik ve doğrulama altyapısı

- Determinism CI: her PR'da SBF vs native 1M tick eşitliği + golden
  vector'ler (kısmen var, CI'a bağla).
- Fuzzing: tick fonksiyonlarına ve referee state machine'ine
  cargo-fuzz; bisection invariant'ları için property testler.
- [ARŞ] Referee state machine'inin model checking'i (TLA+ ya da
  basit exhaustive simülatör — durum uzayı küçük).
- Üçüncü parti audit (mainnet öncesi zorunlu). [GELİR-gider]
- Verifiable build: oyun programının on-chain bytecode'unun repo'daki
  kaynaktan üretildiğinin kanıtı (solana-verify).

---

## 2. Geliştirme Ağacı — Engine, SDK, Servisler

### 2.1 tickproof-sdk (Rust, oyun geliştirici kiti)

Hedef: bir oyun stüdyosunun `TickLogic` yazıp 1 saatte devnet'te
dispute-edilebilir oyun çalıştırması.

- `#[derive(TickState)]` benzeri makrolarla state layout + LoadState/
  Verdict/Tick wrapper'ının otomatik üretimi (arena-program'daki
  boilerplate'i sıfırlar).
- Hazır test harness: golden test, determinizm testi, CU bütçe testi,
  "positions stay in bounds" tarzı invariant makroları.
- Fixed-point matematik kütüphanesini büyüt: trig tabloları, vektör
  tipleri, basit collision primitives (hepsi deterministik, tablo
  tabanlı).
- `cargo tickproof new my-game` şablonu.

### 2.2 TypeScript SDK

- `@tickproof/client`: match/session decode, instruction builder'lar,
  genesis claim (web/lib zaten ilk versiyonu — pakete çıkar).
- Checkpoint izleme + otomatik challenge tetikleme (browser'da hafif
  watchtower).
- [ARŞ] Engine'in WASM'a derlenmesi: sBPF interpreter'ı (solana-sbpf)
  wasm'da koşturup tarayıcıda *seyirci* doğrulaması (truth kaynağı
  değil, UX için canlı replay).

### 2.3 Session Orchestrator [GELİR]

Maçları gerçek hayatta döndüren servis:

- Matchmaking + lobby, WebSocket input relay (iki oyuncuya da canlı
  input akışı → DA sorunu pratikte çözülür).
- Checkpoint poster: belirli aralıklarla referee'ye assertion atan,
  finalize eden bot.
- Operatör SaaS: oyun stüdyoları kendi orchestrator'ını işletmek
  istemez; "verifiable backend as a service". Stüdyo hile yapamaz —
  yapsa bile oyuncular kanıtlayabilir; bu *satış argümanının kendisi*.
- Self-host edilebilir açık kaynak çekirdek + ücretli yönetilen sürüm.

### 2.4 Watchtower [GELİR]

- Bağımsız servis: bound session'ları izler, yalan assertion görünce
  otomatik challenge + bisection oynar (challenger görüşü = kendi
  engine replay'i).
- Gelir: abonelik (maç başına mikro-ücret) ya da kazanılan bond'lardan
  pay. "Sigorta" gibi pazarlanır: *uyurken bile paran güvende*.
- İlk sürüm: devnet-match'teki dispute driver'ın daemon'laştırılması.

### 2.5 Replay Explorer / Debugger

- Web'de tick-by-tick state inspection, input timeline, claim/root
  görselleştirme, dispute'ların "mahkeme kaydı" görünümü.
- Geliştirici için: iki trace'in diverge ettiği ilk tick'i bulan diff
  aracı (determinizm hatası avı).
- arena-viewer'ın web karşılığı (canvas render).

---

## 3. Geliştirme Ağacı — Oyun Katmanı

### 3.1 Oyun şablonları (her biri ayrı iş paketi)

| oyun | state | neden |
|---|---|---|
| pong | ~100 B | en saf 2-oyunculu skill oyunu, demo şampiyonu |
| snake duel | ~1 KB | grid determinizmi, kalabalık state |
| micro-RTS | 4–8 KB | partial-state replay'i zorlar, "ciddi oyun" sinyali |
| turn-based taktik | küçük | input posting yolu, mobil dostu |
| racing time-trial | küçük | tek oyunculu → leaderboard ürünü |

### 3.2 Gizli bilgi problemi [ARŞ — ayrı araştırma hattı]

Poker, fog-of-war, kapalı el: state'in tamamı commit edilirse rakip
göremese de *dispute'ta* açılır. Yollar:

- Commit-reveal (basit, gecikme ekler)
- Her oyuncunun privat state'inin ayrı merkle subtree'si + sadece
  kendi subtree'sine imzalı erişim
- zk yardımı sadece gizlilik için (doğruluk hâlâ native replay) —
  "zk minimal, replay maksimal" hibrit
- Mental poker protokolleri (ağır; en sona)

Fog-of-war çözülürse pazar devasa büyür (strateji oyunları, kart
oyunları). Bu, tickproof'un ikinci "keşif" makalesi olabilir.

### 3.3 AI Agent Arena ★ (hype alanı) [GELİR]

Botlar/AI agent'lar maç yapar, insanlar stake eder:

- Agent'ın kendisi de SBF'e derlenebilir (policy = deterministik
  fonksiyon) → **agent'ın hamlesi bile verifiable**. "Benim botum"
  iddiasının kanıtı zincirde.
- Sezonluk ligler, ödül havuzları, bot NFT'leri.
- AI-agent anlatısı + verifiable execution kesişimi şu an boş — ilk
  hareket avantajı büyük.

---

## 4. Üzerine İnşa Edilebilecek Ürünler

### 4.1 Yakın menzil (mevcut programlarla)

- **P2P wager platformu** [GELİR]: web console'un ürünleşmiş hâli;
  lobi, eşleşme, oynanış, settle. Gelir: pot başına protokol ücreti
  (bps) — wager programına `fee_bps + fee_vault` alanı (tek iş paketi).
- **Turnuva programı** [GELİR]: bracket PDA'sı, giriş ücreti havuzu,
  her maç bir wager match'i, kazanan bracket'ta ilerler. Gelir: rake.
- **On-chain liga + ELO**: settle olayları ELO PDA'sını günceller;
  sezon ödül havuzu. Skill-matchmaking için temel.
- **Leaderboard / time-trial ürünü**: tek oyunculu koşu, sonuç referee
  ile kanıtlanır, haftalık ödül havuzu. (PvP'den daha az yasal risk,
  daha geniş kitle.)

### 4.2 Orta menzil

- **Verdict standardı**: `LoadState(1) / Tick(0) / Verdict(2)`
  arayüzünü SIMD-vari bir spec olarak yayınla; üçüncü parti oyunlar
  escrow/turnuva/watchtower'ı değişiklik olmadan kullansın. Ağ
  etkisinin anahtarı.
- **Spectator parimutuel** [GELİR][YASAL-RİSK]: maç sonucuna seyirci
  bahsi, havuz oranlı. Hukuki analiz şart; bazı yargılarda skill-gaming
  istisnası PvP'ye uyar ama seyirci bahsine uymaz. [KARAR]
- **Replay NFT / moment'ler**: kanıtlanmış maçların replay'i mint
  edilir; "bu skor gerçekten atıldı"nın kanıtı içinde.
- **White-label B2B** [GELİR]: Web2 skill-gaming şirketlerine (chess
  siteleri, e-spor platformları) "provably fair" arka uç. Onların
  sorunu regülasyon + kullanıcı güveni; tickproof ikisine de cevap.

### 4.3 Uzun menzil (oyun dışı — "verifiable compute" olarak tickproof)

`TickLogic` = saf state geçişi. Oyun olması şart değil:

- **Verifiable order matching**: off-chain orderbook motoru, eşleşme
  kuralları SBF'te; "beni atladın" iddiası tek-tick replay ile çözülür.
- **Auction/açık artırma motorları**, **risk motorları** (likidasyon
  sıralaması kanıtlanabilir), **on-chain oyun ekonomisi simülasyonları**.
- **Agent workflow doğrulaması**: deterministik agent adımlarının
  hesap verebilir yürütülmesi.
- Bu hat tutarsa tickproof bir oyun motoru değil, **Solana'nın
  optimistic coprocessor'ü** olur. [KARAR — isimlendirme/anlatı bunu
  ne zaman öne alır?]

---

## 5. Gelir Modeli Özeti

| kaynak | mekanizma | faz |
|---|---|---|
| protokol ücreti | settle'da pot üzerinden 50–100 bps, fee vault | P2 |
| turnuva rake | giriş havuzundan % | P2 |
| orchestrator SaaS | yönetilen session/checkpoint servisi, maç başı ücret | P2–P3 |
| watchtower aboneliği | maç başı mikro-ücret ya da bond payı | P2 |
| B2B white-label | lisans + entegrasyon + SLA | P3 |
| agent arena | sezon ücretleri, ödül havuzu payı | P3 |
| grants/hackathon | Solana Foundation, Colosseum | P0–P1 (hemen) |
| token [KARAR] | sadece gerçek işlevi olursa: watchtower staking / challenger sigorta havuzu / fee switch. Erken token = dikkat dağıtıcı. | P4 |

İlke: **ücret, güvenin değil hizmetin karşılığı olmalı.** Settlement'ın
kendisi trustless kalır; para, kolaylaştıran servislerden kazanılır.

---

## 6. Fazlı Roadmap (ağaç)

Gösterim: `─` iş paketi, `★` kritik yol, `[K]` karar kapısı.

```
P0  SAĞLAMLAŞTIRMA (şimdi → temel güven)
├─★ CI: build-sbf + tüm testler + clippy + determinism (SBF≡native)
├─★ Referee/wager birim test kapsamını genişlet (negatif yollar, fuzz)
├── Mainnet parametre dokümanı (pencereler, bond ekonomisi, saldırılar)
├── Verifiable build (solana-verify) + program upgrade politikası
└── Colosseum/grant başvurusu (mevcut demo + makale yeterli)  [GELİR]

P1  GÜVEN MODELİNİ TAMAMLA (input authenticity ★)
├─★ 1.1A imzalı input entries (tick-core trait + ed25519 introspection)
├─★ Engine + devnet-match entegrasyonu, adversarial test: input uydurma
├── Referee v2: multi-challenger, PDA session'lar
├── Coop settle'a durable nonce (web console UX)
└─[K] DA stratejisi: "iki taraf log tutar" yeterli mi, DA oyunu P3'e mi?

P2  ÜRÜN (para kazanan ilk şeyler)
├─★ Wager v2: fee_bps + fee vault + PDA match'ler  [GELİR]
├─★ Pong (gerçek oynanabilir oyun) + web'de canlı oynanış
├── Turnuva programı (bracket + rake)  [GELİR]
├── Watchtower daemon v1  [GELİR]
├── Session orchestrator v1 (lobby + WS relay + checkpoint poster)
├── TS SDK paketi (@tickproof/client) + Verdict standard dokümanı
└─[K] İlk hedef kitle: kripto-native degens mi, bot yarışları mı?

P3  ÖLÇEK + PLATFORM
├─★ Partial-state replay (access list + merkle proof'lu replay)  [ARŞ]
├── Tick batching (k-tick replay, bisection kısaltma)
├── Micro-RTS şablonu (partial-state'i kanıtlar)
├── AI Agent Arena v1 (SBF policy botları + sezon ligi)  [GELİR][HYPE]
├── Orchestrator SaaS (yönetilen sürüm)  [GELİR]
├── Audit + mainnet-beta (önce düşük limitli)
└─[K] Gizli bilgi hattına yatırım (poker sınıfı) — ayrı track açılır mı?

P4  GENELLEŞME ("Solana'nın optimistic coprocessor'ü")
├── Oyun-dışı ilk vaka: verifiable order matching PoC
├── Fog-of-war / privat state araştırması ürünleşir  [ARŞ]
├── B2B white-label paketi  [GELİR]
└─[K] Token/staking ekonomisi — ancak watchtower ağı gerçekse
```

### Kritik yol (en kısa "savunulabilir ürün" zinciri)

CI → imzalı input'lar → wager fee → pong → watchtower.
Bu beşi bitince: para kazanan, hile yapılamayan, izlenen gerçek bir
oyun var demektir. Gerisi büyütme.

---

## 7. Riskler ve Dürüst Notlar

- **Yasal:** P2P skill wagering çoğu yargıda "beceri oyunu" istisnasına
  girer ama çizgi yargıya göre değişir; seyirci bahsi (4.2) apayrı bir
  rejim. Mainnet + gerçek para öncesi hukuk görüşü şart. Time-trial /
  ödüllü leaderboard en düşük riskli giriş.
- **Platform riski:** Agave CU fiyatlandırması, loader değişiklikleri,
  sBPF sürümleri (v0→v3) replay eşitliğini etkileyebilir. Mollusk'u
  cluster sürümüne pinleme disiplini (Cargo.toml'da zaten not düşülü)
  korunmalı; her cluster upgrade'inde determinism suite koşmalı.
- **Rakip:** MagicBlock ekosistem desteğiyle hızlı; bizim cevabımız
  güven modeli farkı + Verdict standardının açıklığı. Kopyalanırsak:
  ilk hareket + test altyapısı + standart sahipliği.
- **Determinizm tek hata noktası:** tek bir float sızıntısı, tek bir
  sıralama bağımlılığı her şeyi kırar. `#![deny(float_arithmetic)]`
  tarzı korkuluklar SDK'da zorunlu olmalı; golden vector'ler asla
  güncellenmez, regresyon düzeltilir (repo kuralı zaten böyle).
- **Tek geliştirici yükü:** P2'den itibaren orchestrator/watchtower
  operasyon işidir; otomasyonsuz büyütülmemeli.

---

## 8. Hemen Sıradaki 5 Oturum (Opus'a verilecek işler)

1. **CI**: GitHub Actions — build-sbf üç program, cargo test (mollusk
   suite dahil), clippy, web build, check:merkle. Determinism job'ı
   ayrı (1M tick, nightly).
2. **Fuzz + negatif testler**: referee ve wager instruction'larına
   arbitrary-bytes fuzz harness'i; her custom error yoluna birim test.
3. **İmzalı input entries — tasarım dokümanı + tick-core trait'i**
   (kod: native doğrulama + testler; ed25519 introspection sonraki
   oturum).
4. **Wager v2 — fee_bps + fee vault + PDA match hesapları** (devnet'e
   redeploy + devnet-match güncellemesi + web console güncellemesi).
5. **Pong**: `games/pong` + program wrapper + verdict + golden testler +
   arena-viewer'a pong modu. (Web'de oynanabilirlik P2'nin ayrı işi.)

---

*Bu doküman canlıdır; her faz kapısında güncellenir. Ölçmediğin şeyi
roadmap'e yazma kuralı geçerli: buradaki tüm mevcut-durum sayıları
devnet'te yeniden üretilebilir.*
