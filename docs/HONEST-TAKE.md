# tickproof — Dürüst Bakış

> ROADMAP.md satış dokümanına yakındır; bu doküman değildir. Burada
> projeyi bir yatırımcının, bir rakibin ve bir denetçinin gözüyle,
> nezaket filtresi olmadan değerlendiriyorum. Bazı kısımlar moral
> bozabilir; hepsi düzeltilebilir şeyler, o yüzden yazıyorum.

---

## 1. Gerçekten iyi olan ne?

**Çekirdek fikir sağlam ve zarif.** "L1 zaten SBF çalıştırıyor, o halde
fraud proof bir CPI'dan ibaret" gözlemi gerçek bir mühendislik
içgörüsü. 19k CU'luk one-step proof ile 280k CU'luk SP1 verify
arasındaki fark retorik değil, ölçüm. Bu karşılaştırmayı yapabilen
(interp-bench, root-bench yazıp sayı üreten) proje sayısı çok az.

**Disiplin var.** Golden vector'ler, "sabiti güncelleme, regresyonu
düzelt" kuralı, SBF≡native bit-eşitlik testleri, mollusk'u cluster
sürümüne pinleme notları — bunlar olgun mühendislik refleksleri.
Çoğu kripto projesi bu hijyene hiç ulaşamıyor.

**Uçtan uca çalışıyor.** Devnet'te gerçek adversarial settlement 20
transaction'da kapandı. "Whitepaper'ı var ama kodu yok" kategorisinde
değilsin; tam tersi kategoridesin. Bu nadir ve değerli.

---

## 2. "Keşif" iddiası hakkında dürüst olalım

Bisection + tek-adım yeniden yürütme **icat değil** — Arbitrum'un
dispute protokolü, Cartesi, Truebit hepsi bu aileden. Yenilik,
Solana'ya özgü kısayolda: rollup'lar kendi VM'lerini L1'de taklit
etmek zorunda (interpreter, zk devresi); Solana'da hedef VM ile host
VM aynı olduğu için o koca katman buharlaşıyor.

Bu hâlâ değerli bir sentez — ama "kimsenin aklına gelmemiş" varsayımıyla
ilerleme. Ben bu fikrin daha önce uygulanmış halini bilmiyorum, fakat
bilmiyor olmam yokluğunun kanıtı değil. Yapılacak iş: ciddi bir prior
art taraması (forum yazıları, hackathon projeleri, akademik). Birisi
yapmışsa bile kötü haber değil — senin elinde ölçümleriyle çalışan bir
implementasyon var; ama anlatıyı "ilk biziz"den "en iyi ölçülmüş biziz"e
çevirmen gerekebilir.

---

## 3. "MagicBlock'tan iyi olacak" cümlesi hakkında

Teknik güven modeli olarak evet, iddialıyım: kanıt > TEE güveni.
Ama **pazar "daha trustless" olana değil, dağıtımı olana gider.**
MagicBlock'un parası, ekibi, Solana Foundation ilişkileri ve entegre
müşterileri var. Senin bir tane oynanabilir oyunun bile yok henüz —
arena bir test fixture'ı, oyun değil.

Gerçekçi çerçeve şu: MagicBlock *latency* satıyor, sen *hesap
verebilirlik* satıyorsun. Bunlar farklı ürünler ve muhtemelen farklı
müşteriler. "MagicBlock killer" anlatısı seni doğrudan kaybedeceğin
bir karşılaştırmaya sokar (ekosistem desteği yarışı); "MagicBlock'un
üstüne de takılabilen doğrulama katmanı" anlatısı seni herkesin
dostu yapar. Egonu değil konumlanmayı seç.

---

## 4. Henüz kimsenin sormadığı zor sorular

Bunlar roadmap'te var ama ağırlıklarını netleştireyim:

**a) Zaman içinde determinizm — en az konuşulan en derin risk.**
Maç bugün oynanır, dispute üç gün sonra açılır. Replay, *bugünkü*
cluster runtime'ında değil, *dispute anındaki* runtime'da koşar.
Arada bir feature activation sBPF semantiğini, syscall davranışını
veya CU muhasebesini değiştirirse, dürüst operatör kaybedebilir.
Mainnet'te pencereler saatlere çıkınca bu pencere büyür. Çözüm
yönleri: kısa dispute pencereleri, cluster upgrade takvimiyle uyum,
"runtime version" alanını session'a yazmak, her activation öncesi
determinism suite. Bu konuyu ROADMAP'e ben de yeterince sert
yazmadım; ciddiye al.

**b) Netcode gerçeği.** 60Hz P2P oyunda iki oyuncunun input'ları
canlı akmalı. Rollback netcode, gecikme telafisi, tick senkronu —
bunlar oyun endüstrisinin kendi başına zor alanı ve tickproof'un
şu anki engine'i bunların hiçbirini içermiyor. "Doğrulanabilir" maç
ile "oynanabilir" maç arasında aylar var. Pong'u küçümseme; pong'un
bile netcode'u iş.

**c) Operatörü kim çalıştırır?** İki oyuncu da telefondan bağlanıyorsa
engine nerede koşuyor? Cevap "orchestrator" — yani bir sunucu — yani
liveness ve DA pratikte yine merkezi bir parçaya yaslanıyor.
Trustless'lık settlement'ta korunuyor (bu önemli ve gerçek), ama
"sunucusuz" değilsin ve bunu hiçbir zaman iddia etmemelisin.

**d) Bond ekonomisi naif.** Sabit bond, tek challenger, bisection turu
başına maliyet artışı yok. Zengin bir tarafın fakir tarafı dispute
maliyetiyle yorması (griefing-by-attrition) şu an modellenmemiş.
Mainnet öncesi bir ekonomi/saldırı analizi şart — kod kadar önemli.

---

## 5. Pazar hakkında soğuk gerçekler

**Skill-wagering mezarlığı kalabalık.** Web2'de (Skillz halka açıldı,
%99 değer kaybetti) ve web3'te onlarca deneme var. Sorunlar teknik
değildi: kullanıcı edinme maliyeti, hile *algısı*, ödeme rails'leri,
regülasyon. Trustless settlement bunlardan sadece hile algısını çözer.
"Provably fair" bugüne kadar tek başına kimseye kitle kazandırmadı —
poker sitelerindeki provably-fair dalgası niş kaldı.

**Buna karşılık iki gerçek fırsat görüyorum:**

1. **AI agent arena.** İnsan oyuncu edinmek pahalı; bot yazan
   geliştirici edinmek ucuz ve şu an anlatı rüzgârı arkanda. "Botun
   da hamlesi verifiable" hikâyesi içerik üretir (her maç bir
   leaderboard, her dispute bir drama). En yüksek hype/maliyet oranı
   burada.
2. **"Optimistic coprocessor" hattı.** Oyun, teknolojinin demo'su;
   asıl ürün "deterministik hesaplamayı zincire hesap verdirme" olabilir.
   Order matching, auction, risk motoru müşterisi oyuncudan daha az
   ama daha derin cepli. Uzun satış döngüsü, ama oyundan gelen demo
   ("bak, adversarial ortamda bile çalışıyor") satışın kendisi.

**Doğrudan gelir beklentisini düşük tut.** İlk 12 ayın gerçekçi gelir
kaynağı grant + hackathon ödülleri. Protokol fee'si ancak hacim olursa
anlamlı; hacim ancak oynanabilir oyun olursa olur.

---

## 6. Kod tabanı hakkında (denetçi şapkası)

Prototip kalitesi iyi, ama mainnet'le arasında bilinen mesafeler var:

- Referee tek challenger'lı; resolved session ölüyor. Wager bunu
  per-player slot'larla telafi ediyor ama bu bir yama, çözüm değil.
- Input authenticity yok (biliniyor, ama tekrar: bu kapanmadan gerçek
  parayla tek maç oynanmamalı).
- `bond * 2`, `stake * 2` çarpımlarında overflow pratikte imkânsız ama
  checked math'e geçmek bedava sigorta; audit'te ilk yorum bu olur.
- Rent ve lamport muhasebesi köşe durumları (session'a fazla para
  yollanırsa, payout sonrası artıklar) sistematik test edilmedi.
- Web console'daki coop-settle handoff'u (60 sn blockhash penceresi)
  demo için sevimli, ürün için kırık. Durable nonce şart.
- Tek kişinin (artı AI'ın) yazdığı, kimsenin okumadığı ~2000 satır
  kritik kod var. İkinci bir çift göz — insan — mainnet öncesi
  pazarlık konusu bile değil.

---

## 7. AI ile geliştirme hakkında (meta ama önemli)

Bu projeyi Opus ile büyütecek olman güç ve risk aynı anda:

- AI hızlı kod üretir; **anlayışın kodun gerisine düşerse** proje
  fiilen sahipsizleşir. Her oturum sonunda kendine sor: "bu değişikliği
  kağıt üstünde başkasına anlatabilir miyim?" Anlatamıyorsan geri dön.
- Golden test + determinism suite senin AI'a karşı sigortandır:
  model ne kadar ikna edici olursa olsun, frozen vector yalan söylemez.
  O testleri asla "güncelletme".
- AI'dan ölçüm iste, sıfat değil. "Daha hızlı oldu" değil, "CU şuydu,
  şu oldu". Bu repo'nun kültürü zaten böyle; bozma.
- Büyük mimari kararları (referee v2, DA oyunu, partial-state) tek
  oturumda "kodla bitir" deme. Önce tasarım dokümanı yazdır, bir gün
  beklet, sonra kodlat. Geri alması en pahalı hatalar mimari olanlar.

---

## 8. Net hüküm

- **Teknoloji:** 8/10. Gerçek, ölçülmüş, zarif. Eksikler biliniyor ve
  kapanabilir cinsten.
- **"Keşif" iddiası:** 6/10. Sentez güçlü, prior art taraması yapılmadan
  "ilk" deme.
- **Pazar (oyun/wager):** 4/10. Mezarlık kalabalık; tek başına
  trustless'lık kitle getirmez. Agent arena açısı bunu 6-7'ye çeker.
- **Pazar (coprocessor):** belirsiz ama asimetrik — düşük olasılık,
  yüksek ödül. Bence asıl bilet bu.
- **Bugünkü en doğru kullanım:** Colosseum/grant başvurusu + agent
  arena demosu + makalenin yayılması. Bunlar düşük maliyetli, yüksek
  öğrenmeli hamleler; "şirket mi protokol mü ürün mü" kararını
  bunlardan gelen sinyalle ver.

En kısa özet: **elinde gerçek bir şey var, ama değeri oyun platformu
olmasında değil, Solana'ya yeni bir yetenek kazandırmasında.** Oyunu
vitrin olarak kullan, standardı (Verdict + checkpoint + dispute
arayüzü) asıl ürün olarak büyüt. Vitrinle ürünü karıştırırsan ikisi de
yarım kalır.
