# tickproof — Oyun Dışı: Hangi Kategorilerin Altyapısı Olabilir

> Bu doküman oyunu bir kenara bırakır. tickproof'un çekirdeği aslında
> şu: **deterministik bir hesaplamayı off-chain native hızda çalıştır,
> Merkle commitment'larla bağla, itiraz halinde tek adımı zincirin
> kendisine native replay ile çözdür.** Oyun bunun en zorlu vitrini;
> burada vitrinin arkasındaki motorun başka hangi sektörlerin altyapısı
> olabileceğini dürüstçe sıralıyorum. Her kategoride: ne işe yarar,
> tickproof'un bugünkü hali neyi karşılıyor, hangi gap'i kapatmak gerekir,
> ve "Solana'da ilk" olmak için yaklaşımın nasıl değişmesi gerektiği.

---

## 0. Önce çekirdeği oyundan soyutla

Bugün repo oyun terimleriyle konuşuyor (`TickLogic`, arena, "match").
Genelleşmek için zihinsel yeniden adlandırma:

| oyun terimi | genel terim |
|---|---|
| tick | state transition step |
| game program | transition program (SBF) |
| state root + input chain | commitment (state + giriş kanıtı) |
| checkpoint | committed step boundary |
| dispute / replay | one-step fraud proof |
| referee | optimistic verifier |
| wager escrow | settlement / payout hook |

Yani tickproof = **Solana için optimistic, native-replay'li bir
coprocessor**. "Coprocessor" kelimesi burada anahtar: hesabı zincir
*dışında* yaparsın, zincir sadece sonucu *hesap verebilir* kılar.
Bu çerçeveyle aşağıdaki kategoriler açılır.

### Genel uygunluk testi (her kategoriye uygula)

Bir iş yükü tickproof'a uyar mı? Beş soru:

1. **Deterministik mi?** Aynı girdi → aynı çıktı, float/saat/entropi
   bağımlılığı yok. (Değilse en kritik blok burada.)
2. **Saf state geçişi olarak yazılabilir mi?** `state' = f(state, in)`.
3. **State sınırlı ve serileştirilebilir mi?** (Büyükse partial-state
   replay gap'i devreye girer.)
4. **Off-chain hız gerçekten gerekli mi?** Gerekmiyorsa doğrudan
   on-chain yap, tickproof gereksiz karmaşa olur.
5. **İtirazın bir karşı tarafı var mı?** Optimistic model bir
   challenger'a ihtiyaç duyar; "kimin umurunda" olan hesaplamada fraud
   proof'u kim tetikleyecek? (Bu çoğu kategorinin gizli zorluğu.)

Bu beşincisi en çok atlanan: **fraud proof'un ekonomik bir izleyicisi
olmalı.** Oyunda rakip doğal challenger'dır. Oyun dışında "challenger
kim ve neden zahmet etsin" sorusunu her kategori için ayrı cevaplamak
gerekir. Doküman boyunca buna `[challenger?]` ile işaret ediyorum.

---

## 1. DeFi — Off-chain Matching Engine'ler (en güçlü aday)

### 1.1 Central-limit orderbook (CLOB) eşleştirme

**Ne:** Orderbook ve matching off-chain çalışır (hız + ücret), ama
"emrimi atladın / yanlış fiyattan eşledin / öne geçtin (front-run)"
iddiası tek-adım replay ile çözülür. Matching kuralları SBF'te
deterministik bir fonksiyon; bir eşleşme turu = bir "tick".

**Bugün ne karşılanıyor:** Çekirdek aynen oturuyor — sıralı girdi
(emir akışı), deterministik geçiş (price-time priority matching),
state commitment (orderbook Merkle root). dYdX'in off-chain orderbook +
on-chain settlement modeli zaten kanıtlanmış bir pazar; eksik olan
"matching'in doğruluğunun kanıtlanabilirliği".

**Gap'ler:**
- Orderbook state'i oyun state'inden büyük → **partial-state replay
  şart** (ROADMAP 1.4). Tüm kitabı tx'e koyamazsın.
- Emir authenticity = input authenticity gap'inin ta kendisi (ROADMAP
  1.1). İmzalı emirler zaten finansta standart, bu yüzden burada
  *daha kolay* — emirler doğası gereği imzalı gelir.
- `[challenger?]` Güçlü: zarar gören trader doğal challenger. Front-run
  edilen, atlanan emir sahibinin parasal teşviki net.

**Solana'da ilk olmak için yaklaşım:** Genel "coprocessor" deme,
**"verifiable matching engine"** olarak konumlan. Phoenix (on-chain
CLOB) ve off-chain rakipleri arasında üçüncü yol: "off-chain hız, ama
matching'i kanıtlanabilir". Bu spesifik çerçeve Solana'da boş.

### 1.2 Likidasyon ve risk motorları

**Ne:** Perp/lending protokollerinde "hangi pozisyon önce likide edilir,
hangi fiyattan" mantığı off-chain koşar; haksız/sıralama-manipülatif
likidasyon iddiası replay ile denetlenir.

**Gap:** Oracle bağımlılığı determinizmi kırar — fiyat girdisi *input*
olarak commit edilmeli (hangi oracle, hangi slot, hangi imza). Oracle
fiyatını input chain'e bağlamak yeni bir alt-problem.
`[challenger?]` Likide edilen kullanıcı doğal challenger.

### 1.3 Batch auction / frequent batch auction

**Ne:** MEV'i azaltmak için emirleri batch'leyip tek clearing fiyatından
eşle. Clearing fiyatı hesabı deterministik → "yanlış clearing fiyatı"
replay'lenebilir. CoW Protocol'ün solver'larına benzer ama solver'ın
çözümü *kanıtlanabilir doğru*.

**Gap:** Solver rekabeti + doğruluk kanıtı kombinasyonu; "en iyi çözüm"
öznel ama "geçerli çözüm" objektif — tickproof ikincisini garanti eder.

---

## 2. Auction & Mekanizma Tasarımı

**Ne:** Karmaşık açık artırmalar (combinatorial, Vickrey, Dutch,
sealed-bid) off-chain hesaplanır, kazanan/fiyat belirleme deterministik
fonksiyon, "yanlış kazanan seçildi" replay'lenir.

**Bugün:** Mekanizma = saf fonksiyon, tam oturuyor. Sealed-bid için
commit-reveal (ROADMAP 3.2 gizli bilgi hattıyla örtüşür).

**Gap'ler:**
- Sealed-bid'de gizlilik: teklifler reveal'a kadar gizli kalmalı ama
  dispute'ta açılır → commit-reveal veya privat-subtree yaklaşımı.
- `[challenger?]` Kaybeden teklif sahipleri; teşvik orta (zaten
  kaybettiler, dava açma motivasyonu para iadesine bağlı).

**İlk olmak için:** NFT/RWA satışları için "provably fair auction"
primitive'i. Solana NFT pazarı büyük; "açık artırmanın hilesiz
yürüdüğünün kanıtı" satılabilir bir özellik.

---

## 3. Verifiable AI / Agent Execution (en yüksek hype, en belirsiz)

### 3.1 Deterministik agent policy yürütme

**Ne:** Bir AI agent'ın *karar politikası* deterministik bir fonksiyona
indirgenebiliyorsa (öğrenilmiş ağırlıklar sabit, inference integer/
fixed-point), agent'ın her hamlesi SBF'te replay'lenebilir. "Bu agent
gerçekten bu kararı bu girdiyle verdi" kanıtlanır.

**Bugün:** Küçük policy ağları (quantized, integer inference) prensipte
SBF'e derlenebilir. Arena oyununda bot zaten deterministik fonksiyon.

**Gap'ler — burada dürüst olmak şart:**
- Gerçek LLM inference deterministik değil ve SBF'e sığmaz. Bu hat
  sadece **küçük, quantized, deterministik** policy'ler için geçerli.
  "GPT'yi zincirde doğrula" değil; "küçük karar ağını doğrula".
- Floating point ML → integer/fixed-point quantization zorunlu;
  doğruluk kaybı analizi gerekir.
- `[challenger?]` En zayıf nokta: bir agent'ın yanlış davrandığını
  *kim* umursar ve kanıtlamak ister? Cevap: agent'a bahis yapan
  taraf (agent arena), ya da agent'ı kiralayan müşteri (SLA ihlali).

**İlk olmak için:** "Verifiable autonomous agents on Solana" anlatısı
şu an boş ve rüzgâr arkasında. Ama overpromise tuzağı çok büyük —
yapabildiğin şeyi (küçük deterministik policy) net çiz, yapamadığını
(genel LLM) açıkça söyle. Yoksa teknik olarak haklı olup itibar
kaybedersin.

### 3.2 Verifiable inference pipeline'ları (uzun vade)

Preprocessing → quantized model → postprocessing zincirinin her adımı
bir "tick". zkML'in pahalı alternatifine karşı "optimistic ML": çoğu
zaman bedava, itiraz halinde replay. zkML 280k+ CU verify ile pahalıyken,
optimistic yaklaşım happy-path'te neredeyse bedava. **Bu gerçek bir
asimetri** ve araştırmaya değer.

---

## 4. RWA & Compliance Hesaplamaları

**Ne:** Faiz tahakkuku, kupon ödemeleri, waterfall dağıtımları (yapılandırılmış
ürünlerde nakit akışının tranch'lere sıralı dağıtımı), vergi/komisyon
hesapları — hepsi deterministik, kuralları sabit, denetlenebilir olması
*zorunlu*.

**Bugün:** Saf hesaplama; fixed-point matematik kütüphanesi (ROADMAP
2.1) genişletilirse oturur.

**Gap'ler:**
- Düşük frekanslı (günlük/aylık) → "off-chain hız gerekli mi?" testinde
  zayıf. Belki doğrudan on-chain yeterli. tickproof'un avantajı hız değil,
  **karmaşık mantığın ucuz denetlenebilirliği** olur.
- `[challenger?]` Düzenleyici veya zarar gören yatırımcı; teşvik yasal,
  ekonomik değil → optimistic model için zayıf challenger.

**İlk olmak için:** "Auditable computation" çerçevesi. RWA'da satış
argümanı "her hesabın matematiksel kanıtı zincirde" — denetim
maliyetini düşürür. Hız değil, **güvenilirlik + denetim** sat.

---

## 5. On-chain Oyun Ekonomileri & Simülasyon (oyuna komşu ama oyun değil)

**Ne:** Tokenomik simülasyonları, emisyon eğrileri, AMM dışı egzotik
bonding curve'ler, oylama ağırlık hesapları (quadratic/conviction
voting), reputation/credit skorlama — karmaşık deterministik state
makineleri.

**Bugün:** Çok iyi oturuyor; bunlar zaten saf fonksiyon.

**Gap:** Çoğu yeterince ucuz ki doğrudan on-chain yapılabilir.
tickproof ancak hesap *çok* ağırsa (büyük simülasyon, çok adımlı)
değer katar. `[challenger?]` Yönetişimde teşvik karışık.

**İlk olmak için:** Quadratic funding / conviction voting gibi hesabı
ağır mekanizmalar için "verifiable governance compute". Niş ama net.

---

## 6. Veri & Indexing — Verifiable Transformations

**Ne:** Off-chain indexer'lar (The Graph benzeri) ham zincir verisini
işler; "indexer doğru hesapladı mı?" sorusu bugün güvene dayalı.
Deterministik transform → replay ile denetlenebilir.

**Gap'ler:**
- Girdi = zincir verisi; "hangi blok aralığı, hangi state" commitment'ı
  gerekir (zaten zincirde olduğu için kolaylaşır).
- `[challenger?]` Yanlış veriye dayanıp zarar gören tüketici; orta teşvik.

**İlk olmak için:** "Verifiable indexer" — Solana indexing pazarı
büyük ve güven sorunu gerçek. Ama bu, mevcut kod tabanından en uzak
kategori (girdi modeli farklı).

---

## 7. Cross-domain / Coprocessor-as-a-Service (nihai form)

**Ne:** Herhangi bir Solana programı, ağır deterministik bir alt-işi
tickproof'a "outsource" eder: program bir hesap talebi yayınlar, bir
operatör off-chain hesaplar ve checkpoint commit eder, sonuç callback
ile programa döner; yanlışsa challenge penceresi korur.

**Bu, tickproof'u bir SDK/protokolden bir Solana primitive'ine çevirir.**
Axiom (Ethereum coprocessor) ve Bonsol (Solana'da RISC0 zk
coprocessor) bu alanda; ikisi de zk tabanlı. tickproof'un farkı:
**zk yok, optimistic + native replay** → happy-path'te dramatik daha
ucuz, ama interaktif challenge ve liveness varsayımı gerektirir.

**Gap'ler:**
- Callback/async sonuç teslimi deseni yok (yeni mimari).
- Liveness: sonucu kim ne zaman teslim eder, gecikirse ne olur.
- `[challenger?]` Genel coprocessor'da en akut: rastgele bir hesabı
  kim izler? → "verifier-as-a-service" / watchtower ağı şart olur
  (ROADMAP 2.4 ile birleşir).

**İlk olmak için:** "Solana'nın ilk optimistic coprocessor'ü"
(zk-olmayan). Bonsol/Axiom'a karşı net teknik diferansiyasyon:
zk-coprocessor pahalı-ama-non-interaktif; optimistic-coprocessor
ucuz-ama-interaktif. İkisi farklı trade-off; pazar ikisini de barındırır.
Bu muhtemelen **en büyük "ilk"** ve en zor olan.

---

## 8. Dürüst Önceliklendirme

Beş soruluk uygunluk testi + `[challenger?]` gücü + mevcut koda yakınlık
ile sıralarsam:

| kategori | teknik uyum | challenger gücü | koda yakınlık | hype | net |
|---|---|---|---|---|---|
| CLOB matching (1.1) | yüksek | güçlü | yüksek | orta | **en sağlam** |
| Batch auction (1.3) | yüksek | güçlü | yüksek | orta | güçlü |
| Auction/NFT (2) | yüksek | orta | yüksek | orta | güçlü |
| Coprocessor (7) | yüksek | zayıf* | orta | yüksek | **en büyük bahis** |
| Agent execution (3.1) | orta | orta | yüksek | çok yüksek | asimetrik |
| Likidasyon (1.2) | orta | güçlü | orta | orta | sağlam ama oracle gap |
| Oyun ekonomi (5) | yüksek | zayıf | yüksek | düşük | niş |
| RWA (4) | orta | zayıf | orta | orta | "hız gerekli mi?" zayıf |
| Indexing (6) | orta | orta | düşük | düşük | uzak |

\* watchtower ağı kurulursa challenger sorunu çözülür.

**Okuma:** En sağlam *ilk* hamle finansal matching (1.1) — çünkü
challenger doğal (zarar gören trader), emirler zaten imzalı (input
auth gap'i hafif), ve "verifiable CLOB" Solana'da boş bir konum.
En büyük *vizyon* coprocessor (7) — ama challenger/liveness problemini
çözmek için watchtower ağını önce kurman gerekir. Agent execution (3.1)
en yüksek hype ama overpromise riski en yüksek; yapabildiğini net çiz.

---

## 9. Hangi gap'ler "oyun dışı"nın kapısını açar?

Oyun dışına geçmek için ROADMAP'teki şu üç iş paketi *önkoşul*:

1. **Partial-state replay (ROADMAP 1.4).** Orderbook, simülasyon, büyük
   state olan her şey bunu bekler. Bu olmadan tickproof "küçük state"
   kutusunda kalır. **En yüksek kaldıraçlı tek iş.**
2. **Input authenticity + imzalı/zaman damgalı girdiler (ROADMAP 1.1).**
   Finansta emir imzası, oracle imzası, kullanıcı imzası — hepsi bu
   gap'in çözümüne bağlı. Finansta daha kolay (imza zaten var) ama şart.
3. **Watchtower / verifier ağı (ROADMAP 2.4).** Oyunda rakip challenger;
   oyun dışında `[challenger?]` sorusunun tek genel cevabı bağımsız
   izleyici ağı. Coprocessor vizyonu buna bağlı.

Bir de yeni eklenmesi gereken, ROADMAP'te zayıf duran:

4. **Async sonuç/callback deseni.** Coprocessor olmak istiyorsan
   "talep → off-chain hesap → checkpoint → callback → challenge
   penceresi" mimarisi yok. Bu yeni bir tasarım hattı.
5. **Oracle/dış girdi commitment'ı.** DeFi kategorilerinin çoğu dış
   fiyat verisine bağlı; bunu determinizmi bozmadan input chain'e
   bağlamak çözülmemiş bir alt-problem.

---

## 10. "İlk olmak" üzerine dürüst söz

- **Global ilk değilsin:** optimistic fraud proof (Arbitrum), interaktif
  bisection (Cartesi/Truebit), coprocessor (Axiom/Bonsol) — hepsi var.
- **Solana'da bazı şeylerde ilk olabilirsin:** "zk-olmayan optimistic
  coprocessor", "verifiable off-chain matching", "verifiable
  deterministic agent" çerçevelerinin Solana'da net bir sahibi yok.
- **İlk olmasan bile fark yaratabileceğin eksen:** native replay'in
  ucuzluğu (zk'ya karşı happy-path'te 10-15x daha ucuz). Bu retorik
  değil ölçüm; konumlanmanı "biz ilkiz" yerine "biz en ucuz happy-path"
  üstüne kur. İlk olmak kırılgan; ölçülebilir üstünlük değil.

**En dürüst tek cümle:** tickproof'un asıl değeri yeni bir fikir
olmasında değil, *Solana'nın native SBF yürütmesini bir doğrulama
substratı olarak kullanan ilk pratik, ölçülmüş implementasyon* olma
ihtimalinde. Oyun bunu ispatlayan vitrin; yukarıdaki kategoriler ise
aynı motorun nereye kadar gidebileceğinin haritası. Hangisine
gireceğini, partial-state + input-auth + watchtower üçlüsünü
bitirdikten sonra gelen sinyalle seç — o üçlü bitmeden kategori
seçimi erken.
