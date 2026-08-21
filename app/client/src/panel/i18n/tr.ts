import type { TranslationKey } from "@/panel/i18n/en"

export const tr: Record<TranslationKey, string> = {
  "actions.cancel": "Vazgeç",
  "actions.save": "Kaydet",
  "actions.create": "Oluştur",
  "actions.edit": "Düzenle",
  "actions.delete": "Sil",
  "actions.close": "Kapat",
  "actions.back": "Geri",
  "actions.continue": "Devam et",
  "actions.retry": "Yeniden dene",
  "actions.saving": "Kaydediliyor…",

  "general.areYouSure": "Emin misiniz?",
  "general.unsavedTitle": "Kaydedilmemiş değişiklikler var",
  "general.unsavedDescription":
    "Şimdi çıkarsanız yazdıklarınız kaybolur. Bu geri alınamaz.",
  "general.noValue": "—",
  "general.metadata": "Üstveri",
  "general.json": "JSON",
  "general.details": "Ayrıntılar",
  "general.loading": "Yükleniyor…",
  "general.empty": "Burada henüz bir şey yok",
  "general.of": "/",

  "error.unreachable": "Hiçbir sunucu yanıt vermedi.",
  "error.unauthenticated": "Bu panel bağlı değil.",
  "error.denied": "Sunucu bunu reddetti.",
  "error.notFound": "Bulunamadı.",
  "error.refused": "Sunucu bu isteği reddetti.",
  "error.drifted": "Sunucunun yanıtını bu panel okuyamıyor.",

  "nav.group.selling": "Satış",
  "nav.group.orders": "Siparişler",
  "nav.group.gettingItThere": "Teslimat",
  "nav.group.money": "Para",
  "nav.group.theShop": "Mağaza",
  "nav.group.host": "Ayrıca burada",
  "nav.group.thisServer": "Bu sunucu",

  "nav.products": "Ürünler",
  "nav.pricing": "Fiyatlandırma",
  "nav.promotions": "Kampanyalar",
  "nav.orders": "Siparişler",
  "nav.baskets": "Sepet grupları",
  "nav.carts": "Sepetler",
  "nav.subscriptions": "Abonelikler",
  "nav.inventory": "Stok",
  "nav.fulfilment": "Gönderim",
  "nav.payments": "Ödemeler",
  "nav.credit": "Bakiye",
  "nav.payouts": "Hakedişler",
  "nav.tax": "Vergi",
  "nav.customers": "Müşteriler",
  "nav.store": "Mağaza ayarları",
  "nav.digital": "Dijital",
  "nav.workflows": "İş akışları",
  "nav.operators": "Kullanıcılar",
  "nav.batch": "İçe ve dışa aktarma",
  "nav.records": "Neler oldu",

  "nav.operations": "{n} işlem",
  "nav.built": "hazır",
  "nav.soon": "yakında",
  "nav.overview": "Genel bakış",
  "nav.goTo": "Git…",
  "nav.disconnect": "Bağlantıyı kes",
  "nav.adminToken": "Yönetici anahtarı — bir kişi değil",
  "nav.coverage":
    "{operations} yönetim işleminin {covered} tanesinin ekranı var",

  "table.back": "Geri",
  "table.next": "İleri",
  "table.chosen": "{count} seçildi",
  "table.showing": "{total} kaydın {shown} tanesi",

  "screen.products.title": "Ürünler",
  "screen.products.subtitle":
    "Bu ekran her durumu görür. Mağaza yalnızca yayımlananları görür.",
  "screen.products.empty": "Ürün yok",
  "screen.products.emptyAny": "Katalogda henüz bir şey yok.",
  "screen.products.emptyStatus": "{status} durumunda bir şey yok.",

  "screen.orders.title": "Siparişler",
  "screen.orders.subtitle":
    "Taslaklar da listelenir ve öyle olduklarını söyler.",
  "screen.orders.empty": "Sipariş yok",
  "screen.orders.emptyAny": "Henüz sipariş verilmedi.",

  "screen.customers.title": "Müşteriler",
  "screen.customers.subtitle":
    "Misafirler de müşteridir — hesap açılmadan önce sepet bir müşteri yaratır.",
  "screen.customers.empty": "Müşteri yok",
  "screen.customers.emptyAny": "Henüz kimse alışveriş yapmadı.",

  "screen.inventory.title": "Stok",
  "screen.inventory.subtitle":
    "Sayılan şey kalemdir. Elde ne olduğu her konum için ayrı sayılır, bir alt kırılımda.",
  "screen.inventory.empty": "Stokta bir şey yok",
  "screen.inventory.emptyAny": "Henüz stok kalemi yok.",

  "screen.carts.title": "Sepetler",
  "screen.carts.subtitle": "Mağazadaki her sepet, terk edilenler dahil.",
  "screen.carts.empty": "Sepet yok",
  "screen.carts.emptyAny": "Henüz kimse sepet açmadı.",

  "screen.promotions.title": "Kampanyalar",
  "screen.promotions.subtitle":
    "Kullanım, sepet ödendiğinde değil, sipariş verildiğinde sayılır.",
  "screen.promotions.empty": "Kampanya yok",
  "screen.promotions.emptyAny": "Sunulan bir şey yok.",

  "screen.subscriptions.title": "Abonelikler",
  "screen.subscriptions.empty": "Abonelik yok",
  "screen.subscriptions.emptyAny": "Yinelenen bir satış yok.",

  "screen.credit.title": "Bakiye",
  "screen.credit.subtitle":
    "Hediye kartları. Müşterinin hesabındaki bakiye kendi kaydından okunur.",
  "screen.credit.empty": "Hediye kartı yok",
  "screen.credit.emptyAny": "Henüz kart çıkarılmadı.",

  "search.nothingMatches": "{q} ile eşleşen bir şey yok.",

  "field.id": "Kimlik",
  "field.title": "Başlık",
  "field.created": "Oluşturuldu",
  "field.updated": "Güncellendi",
  "field.email": "E-posta",
  "field.firstName": "Ad",
  "field.lastName": "Soyad",
  "field.phone": "Telefon",
  "field.company": "Firma",
  "field.sku": "Stok kodu",
  "field.account": "Hesap",
  "field.erased": "Silindi",
  "field.ships": "Gönderim",

  "value.yes": "Evet",
  "value.no": "Hayır",
  "value.registered": "Kayıtlı",
  "value.guest": "Misafir",
  "value.shipped": "Kargolanır",
  "value.digital": "Dijital, kargo yok",

  "detail.customer.title": "Kim olduğu",
  "detail.customer.empty": "Müşteri yok",
  "detail.customer.account": "Hesap",
  "detail.inventory.title": "Kalem",
  "detail.inventory.empty": "Stok kalemi yok",
  "detail.nothingToShow": "Gösterilecek bir şey yok.",

  "field.handle": "Kısa ad",
  "field.subtitle": "Alt başlık",
  "field.description": "Açıklama",
  "field.discountable": "İndirime girer",
  "field.rejectedReason": "Ret gerekçesi",
  "field.productType": "Ürün tipi",
  "field.collection": "Koleksiyon",
  "field.externalId": "Dış kimlik",
  "field.thumbnail": "Küçük görsel",
  "field.weight": "Ağırlık",
  "field.length": "Uzunluk",
  "field.height": "Yükseklik",
  "field.width": "Genişlik",
  "field.material": "Malzeme",
  "field.hsCode": "GTİP kodu",
  "field.originCountry": "Menşe ülke",
  "field.variantId": "Varyant kimliği",

  "detail.product.general": "Genel",
  "detail.product.organisation": "Sınıflandırma",
  "detail.product.media": "Görsel",
  "detail.product.shipping": "Gönderi bilgileri",
  "detail.product.shippingWhy":
    "Taşıyıcının fiyat verebilmesi ve gümrüğün geçirebilmesi için gerekenler.",
  "detail.product.digital": "Dijital içerik",
  "detail.product.digitalWhy":
    "Bir dosya tek bir varyanta aittir — yukarıdaki varyantlardan bir kimlik alın ve neyi taşıdığını görün ya da ekleyin.",

  "general.nothingToShow": "Gösterilecek bir şey yok.",
  "state.noHost": "Yanıt veren bir sunucu yok",
  "state.noHostWhy":
    "tezgah bir kütüphanedir ve kendisi hiçbir şey sunmaz. VITE_TEZGAH_API değerini api::routes() bağlayan bir sunucuya yöneltin ya da örnek dükkânı çalıştırın.",
  "state.noToken": "Bu panelde anahtar yok",
  "state.noTokenWhy":
    "Yönetim yüzeyi bir anahtar ister ve hiçbir şey gönderilmedi. Paneli sunucunun ADMIN_TOKEN değeriyle bağlayın.",
  "state.refused": "Reddedildi",
  "state.refusedWhy":
    "Anahtar gönderildi ve sunucu hayır dedi — yanlış anahtar ya da sunucunun artık tanımadığı bir anahtar. Hangi kayıtların var olduğunu asla söylemez, dolayısıyla bundan çıkarılacak başka bir şey yok.",
  "state.notHere": "Burada yok",
  "state.notHereWhy": "Sunucu yanıt verdi ve böyle bir kayıt yok.",
  "state.refusedRequest": "İstek geçmedi",
  "state.drifted": "Panel ile sunucu aynı şeyi söylemiyor",
  "state.driftedWhy":
    "Sunucu yanıt verdi ama yanıt bu panelin beklediği biçimde değil. Panelin tipleri Rust tarafından elle aktarılır; demek ki sunucu ilerledi, panel ilerlemedi.",
  "empty.basket": "Sepet grubu yok",
  "empty.orders": "Sipariş yok",
  "empty.ordersWhy": "Bu sepet grubu henüz bir siparişe ayrılmadı.",
  "empty.carts": "Sepet yok",
  "empty.cartsWhy":
    "Şu anda hiçbir satıcı kapsamının bu ödemeye ait açık bir bacağı yok.",
  "frame.rejected": "Reddedilenler",
  "frame.rejectedWhy":
    "Başlığın altındaki ilk satırdan başlayarak satır numarasına göre.",
  "empty.giftCard": "Hediye kartı yok",
  "empty.customer": "Müşteri yok",
  "empty.carriers": "Taşıyıcı yok",
  "empty.carriersWhy": "Bir dükkân taşıyıcı açmadan hiçbir şey gönderilmez.",
  "frame.carriers": "Taşıyıcılar",
  "frame.fulfilmentSets": "Gönderim kümeleri",
  "frame.fulfilmentSetsWhy":
    "Bir küme, tek bir taşıyıcının hizmet verdiği bölgeleri gruplar.",
  "empty.fulfilmentSets": "Gönderim kümesi yok",
  "empty.fulfilmentSetsWhy":
    "Bir küme, bir deponun ya da mağazanın gönderim yaptığı hizmet bölgelerini gruplar.",
  "empty.shippingOption": "Gönderim seçeneği yok",
  "frame.optionTypes": "Seçenek türleri",
  "frame.optionTypesWhy":
    "Alıcının arasından seçtiği etiketler — standart, hızlı — seçenekler arasında paylaşılır.",
  "empty.shippingOptionTypes": "Gönderim seçeneği türü yok",
  "frame.shippingOptions": "Gönderim seçenekleri",
  "frame.shippingOptionsWhy":
    "Alıcının kasada seçebilecekleri ve her birinin bedeli.",
  "empty.shippingOptions": "Gönderim seçeneği yok",
  "empty.shippingOptionsWhy":
    "Bir hizmet bölgesi, seçenek eklenene kadar gönderilecek bir yol sunmaz.",
  "empty.shippingProfile": "Gönderim profili yok",
  "frame.shippingProfiles": "Gönderim profilleri",
  "frame.shippingProfilesWhy":
    "Bir seçeneğin neyi taşıyabileceği: birlikte yolculuk eden mallar ve edemeyenler.",
  "empty.shippingProfiles": "Gönderim profili yok",
  "empty.shippingProfilesWhy":
    "Bir ürün bir profille gönderilir; profil, hangi seçeneklerin ona uyduğunu belirler.",
  "frame.invited": "Davet edilenler",
  "frame.invitedWhy":
    "Gönderildi ve henüz kabul edilmedi. Aynı adrese yeniden davet göndermek ikinci bir bağlantı eklemez, mevcut bağlantının yerine geçer.",
  "empty.accounts": "Hesap yok",
  "empty.accountsWhy":
    "Bu yönetim paneline yalnızca yönetici anahtarı ulaşabiliyor. Bir hesap açın.",
  "frame.accounts": "Hesaplar",
  "frame.accountsWhy":
    "Bir hesabı kapatmak, aynı işlem içinde o hesabın tüm oturumlarını sonlandırır.",
  "empty.order": "Sipariş yok",
  "empty.entitlements": "Dijital hak yok",
  "empty.entitlementsWhy": "Bu sipariş hiçbir dijital hak taşımıyor.",
  "empty.payment": "Ödeme yok",
  "frame.payments": "Ödemeler",
  "frame.paymentsWhy":
    "Provizyon almak ile tahsil etmek ayrı işlerdir; var olan bir ödeme henüz alınmış para demek değildir.",
  "empty.payments": "Ödeme yok",
  "empty.paymentsWhy": "Henüz hiçbir tahsilat yapılmadı.",
  "frame.refundReasons": "İade gerekçeleri",
  "frame.refundReasonsWhy":
    "Bir iadenin dayandırılabileceği gerekçeler; raporlarda sayılabilsinler diye tutulur.",
  "empty.refundReasons": "İade gerekçesi yok",
  "empty.refundReasonsWhy":
    "Gerekçesiz de iade yapılabilir, ama burada henüz nedenini açıklayan bir şey yok.",
  "frame.commissionRules": "Komisyon kuralları",
  "frame.commissionRulesWhy":
    "Pazar yerinin satıcının satırından ne kadarını, neye göre alıkoyduğu.",
  "empty.commissionRules": "Komisyon kuralı yok",
  "empty.commissionRulesWhy":
    "Kuralı ve varsayılanı olmayan bir kategoriden komisyon alınmaz — bir kural konana kadar hiçbir şey kesilmez.",
  "empty.payouts": "Hakediş yok",
  "empty.payoutsWhy": "Henüz ödenmiş olarak kaydedilen bir şey yok.",
  "empty.balance": "Bakiye yok",
  "empty.balanceWhy": "Bu para biriminde bir şey yok.",
  "empty.payoutLines": "Hakediş satırı yok",
  "empty.payoutLinesWhy": "Bu siparişten henüz bir kazanç doğmadı.",
  "empty.priceList": "Fiyat listesi yok",
  "frame.priceLists": "Fiyat listeleri",
  "frame.priceListsWhy":
    "Tarihli ya da koşullu fiyatlar — adını koyan bir indirim ya da koymayan bir geçersiz kılma.",
  "empty.priceLists": "Fiyat listesi yok",
  "empty.priceListsWhy":
    "Bir fiyat listesi, eşleştiği kural için fiyat kümesinin kendi fiyatlarının yerine geçer.",
  "empty.pricePreference": "Tanımlı tercih yok",
  "empty.pricePreferenceWhy":
    "Bu niteliğin vergiyi nasıl göstereceğine henüz bir şey karar vermiyor.",
  "empty.priceSet": "Fiyat kümesi yok",
  "empty.prices": "Fiyat yok",
  "empty.pricesWhy": "Bu fiyat kümesinde henüz fiyat yok.",
  "frame.prices": "Fiyatlar",
  "frame.pricesWhy":
    "Bir tutar yazın ve hepsini birlikte kaydedin. Yalnızca tutar değiştirilebilir — bir fiyatı o fiyat yapan şey para birimi ve miktar aralığıdır.",
  "empty.product": "Ürün yok",
  "empty.digitalContent": "Dijital içerik yok",
  "empty.digitalContentWhy": "Bu varyant henüz dosya taşımıyor.",
  "empty.promotion": "Kampanya yok",
  "empty.audit": "Henüz yazılmış bir şey yok",
  "empty.auditWhy": "Bir şey değiştiğinde bir denetim kaydı yazılır.",
  "frame.audit": "Denetim kaydı",
  "frame.auditWhy":
    "Kimin hangi kayda ne yaptığı. ADMIN_TOKEN ile gelen bir istek kimseyi adlandırmaz, çünkü paylaşılan bir sır kişi değildir.",
  "empty.events": "Henüz söylenecek bir şey yok",
  "empty.eventsWhy": "Anlatmaya değer bir şey olduğunda bir olay yazılır.",
  "frame.outbox": "Giden kutusu",
  "empty.currencies": "Para birimi yok",
  "empty.currenciesWhy":
    "Bir dükkân para birimi açmadan ne fiyat verilir ne de sepet açılır.",
  "frame.currencies": "Para birimleri",
  "frame.currenciesWhy":
    "Üs, para biriminin nasıl yazıldığıdır, bir çarpan değil — burada hiçbir şey alt birimde saklanmaz.",
  "frame.keys": "Yayınlanabilir anahtarlar",
  "frame.keysWhy":
    "Bir anahtar, mağaza yüzünü okuyabileceği kanallara sabitler. Değer yalnızca üretildiği anda bir kez gösterilir.",
  "empty.keys": "Yayınlanabilir anahtar yok",
  "empty.keysWhy":
    "Mağaza yüzünün x-publishable-key olarak gönderdiği değer. Üretildiğinde bir kez gösterilir.",
  "empty.region": "Bölge yok",
  "frame.regions": "Bölgeler",
  "frame.regionsWhy":
    "Bölge, tek para birimiyle satış yapılan ve vergi sorusuna tek yanıt veren ülkeler kümesidir.",
  "empty.regions": "Bölge yok",
  "empty.regionsWhy":
    "Bölge, para birimini ve verginin nasıl gösterileceğini belirler.",
  "empty.salesChannel": "Satış kanalı yok",
  "frame.salesChannels": "Satış kanalları",
  "frame.salesChannelsWhy":
    "Bir ürünün nerede satıldığı. Ürün bazı kanallara aittir, bazılarına değil.",
  "empty.salesChannels": "Satış kanalı yok",
  "empty.salesChannelsWhy":
    "Kanal, bir mağaza yüzünün hangi ürünleri görebileceğini belirler.",
  "empty.subscription": "Abonelik yok",
  "empty.taxRate": "Vergi oranı yok",
  "frame.taxRates": "Vergi oranları",
  "frame.taxRatesWhy":
    "Her bölgede bir varsayılan ve üstüne binen birleşebilir oranlar.",
  "empty.taxRates": "Vergi oranı yok",
  "empty.taxRatesWhy": "Bir oran tanımlanana kadar bölge vergi almaz.",
  "empty.taxRegion": "Vergi bölgesi yok",
  "frame.taxRegions": "Vergi bölgeleri",
  "frame.taxRegionsWhy":
    "İç içedir: bir ilin oranları ülkesinin oranlarının altında durur.",
  "empty.taxRegions": "Vergi bölgesi yok",
  "empty.taxRegionsWhy":
    "Burada bölgesi olmayan bir ülke ya da il vergi almaz.",
  "empty.registrations": "Kayıt yok",
  "empty.registrationsWhy":
    "Dükkân, vergi beyanı için kayıtlı olduğu hiçbir yeri kaydetmemiş.",
  "frame.registrations": "Vergi kayıtları",
  "frame.registrationsWhy":
    "Dükkânın nerede tahsilat için kayıtlı olduğu ve hangi numarayla.",
  "frame.deadLetters": "Vazgeçilen çalışmalar",
  "frame.deadLettersWhy":
    "Yeniden deneme hakkı tükenmiş çalışmalar. Bunları kendiliğinden yeniden deneyen bir şey yok.",
  "empty.deadLetters": "Vazgeçilen çalışma yok",
  "empty.deadLettersWhy":
    "Yeniden deneme hakkı tükenip vazgeçilen bir şey yok.",
  "empty.run": "Çalışma yok",
  "empty.steps": "Adım yok",
  "empty.stepsWhy": "Bu iş akışı hiç adım tanımlamamış.",
  "empty.runs": "Çalışma yok",
  "frame.outboxWhy":
    "Dükkânın söyleyecekleri. Bir hedef tanımlıysa bunlar imzalı isteklerle gönderilir; tanımlı değilse burada okunmak üzere bekler.",

  "connect.choosePassword": "Bir parola seçin",
  "connect.choosePasswordWhy":
    "Bu bağlantı bir kez çalışır. Sonrasında, gönderildiği adresle giriş yapın.",
  "connect.signIn": "Giriş yap",
  "connect.signInWhy": "Bir hesap ya da sunucunun başlatıldığı anahtar.",
  "connect.account": "Hesap",
  "connect.adminToken": "Yönetici anahtarı",
  "connect.password": "Parola",
  "connect.again": "Yeniden",
  "connect.tooShort": "En az on iki karakter.",
  "connect.doNotMatch": "Bu ikisi aynı değil.",
  "connect.setting": "Ayarlanıyor…",
  "connect.setAndSignIn": "Parolayı belirle ve gir",
  "connect.email": "E-posta",
  "connect.signingIn": "Giriş yapılıyor…",
  "connect.noHostAnswered": "Yanıt veren bir sunucu olmadı.",
  "connect.invitationSpent": "Bu davet ya kullanılmış ya da süresi dolmuş.",
  "connect.wrongPassword": "Bu e-posta ve parola bir hesapla eşleşmiyor.",
  "connect.noSelfReset":
    "İsteyebileceğiniz bir sıfırlama bağlantısı yok: parolasını unutan birine yeni parolayı bir sahip belirler; kimse belirleyemiyorsa geri dönüş yolu yönetici anahtarıdır.",
  "connect.tokenPlaceholder": "ADMIN_TOKEN değeri",
  "connect.connect": "Bağlan",
  "connect.tokenWhy":
    "Paylaşılan bir sır, bir kişi değil — onunla yapılan hiçbir değişiklik kimseye atfedilemez. Bununla Hesaplar ekranından bir hesap açın ve sonra onunla girin. Ne anahtar ne hesapla başlatılmışsa sunucu yönetim yüzeyini hiç sunmaz.",

  "field.basketId": "Sepet grubu kimliği",
  "field.csv": "CSV",
  "field.row": "Satır",
  "field.reason": "Gerekçe",
  "field.reasonOptional": "Gerekçe (isteğe bağlı)",
  "value.noneGiven": "Belirtilmemiş.",
  "field.checked": "Doğrulandı",
  "field.where": "Nerede",
  "field.goodFor": "Neyi kapsar",
  "field.certificate": "Belge",
  "sort.newest": "Önce en yeni",
  "sort.byEmail": "E-postaya göre",
  "sort.byTitle": "Başlığa göre",
  "filter.anyRenewal": "Her yenileme",
  "filter.ending": "Bitiyor",
  "filter.renewing": "Yenileniyor",
  "filter.anyCard": "Her kart",
  "filter.spendable": "Harcanabilir",
  "filter.stopped": "Durduruldu",
  "filter.spent": "Tükendi",
  "search.carts": "E-posta ara",
  "filter.anyCart": "Her sepet",
  "filter.stillOpen": "Hâlâ açık",
  "filter.ordered": "Siparişe döndü",
  "filter.anyPayment": "Her ödeme",
  "filter.authorized": "Provizyon",
  "filter.captured": "Tahsil edildi",
  "filter.canceled": "İptal edildi",
  "search.taxRates": "Ad ya da kod ara",
  "filter.anyRate": "Her oran",
  "filter.theDefault": "Varsayılan",
  "filter.stacking": "Üzerine biner",
  "search.priceLists": "Başlık ara",
  "search.shippingOptions": "Ad ara",
  "filter.anyOption": "Her seçenek",
  "filter.outbound": "Alıcının seçtikleri",
  "filter.forReturns": "İadeler için",
  "filter.anyStatus": "Her durum",
  "filter.anyState": "Her durum",
  "field.location": "Konum",
  "field.counted": "Sayılan",
  "field.incoming": "Gelen",
  "field.reserved": "Ayrılan",
  "field.available": "Kullanılabilir",
  "action.invite": "Birini davet et",
  "field.runsOut": "Süresi doluyor",
  "field.newPassword": "Yeni parola",
  "field.downloads": "İndirme",
  "field.granted": "Verildi",
  "field.attribute": "Nitelik",
  "field.priority": "Öncelik",
  "frame.priceRules": "Bu fiyatın neye uygulandığı",
  "empty.rules": "Kural yok.",
  "empty.notLookedUp": "Henüz bir şey aranmadı.",
  "field.key": "Anahtar",
  "field.valid": "Geçerlilik",
  "field.autoGrant": "Kendiliğinden ver",
  "field.added": "Eklendi",
  "action.addFile": "Dosya ekle",
  "field.storageKey": "Saklama anahtarı",
  "field.maxDownloads": "En çok indirme",
  "field.validDays": "Geçerlilik (gün)",
  "field.rank": "Sıra",
  "action.rejectProduct": "Bu ürünü reddet",
  "field.stock": "Stok",
  "field.who": "Kim",
  "field.did": "Ne yaptı",
  "field.what": "Ne",
  "field.about": "Neyle ilgili",
  "field.delivered": "İletildi",
  "actions.copy": "Kopyala",
  "action.cancelSubscription": "Bu aboneliği iptal et",
  "action.stopNow": "Bunun yerine hemen durdur",
  "field.variant": "Varyant",
  "field.unitPrice": "Birim fiyat",
  "field.scheme": "Rejim",
  "field.taxId": "Vergi numarası",
  "field.home": "Merkez",
  "field.whichOne": "Hangisi",
  "field.attempts": "Deneme",
  "field.runAfter": "Şundan sonra çalıştır",
  "field.leaseUntil": "Kilit bitişi",
  "frame.payoutLines": "Siparişe göre hakediş satırları",

  "search.orders": "E-posta ya da sipariş numarası ara",
  "search.customers": "Ad, e-posta, firma ara",
  "search.promotions": "Kampanya kodu ara",
  "search.products": "Başlık, kısa ad, alt başlık ara",
  "placeholder.basketId": "birden çok satıcıya yayılan bir ödemenin kimliği",
  "placeholder.priceSetId": "fiyat kümesi kimliği",
  "placeholder.leftOutPreference": "niteliğin kendi tercihi için boş bırakın",
  "placeholder.orderId": "sipariş kimliği",
  "placeholder.productTitle": "Kot ceket",
  "placeholder.draftDefault": "taslak (varsayılan)",
  "placeholder.keptWithContract": "sözleşmeyle birlikte saklanır",
  "placeholder.keyTitle": "Mağaza yüzü",
  "placeholder.currencyName": "Türk lirası",
  "placeholder.variantId": "varyant kimliği",
  "placeholder.unlimited": "sınırsız",
  "placeholder.neverExpires": "süresi dolmaz",
  "placeholder.or": "ya da…",
  "placeholder.shopsOwn": "boş bırakılırsa dükkânın kendi dili",

  "field.no": "No",
  "field.default": "Varsayılan",
  "field.dunning": "Tahsilat denemesi",
  "field.inStore": "Mağazada",
  "field.initialBalance": "Açılış bakiyesi",
  "field.label": "Etiket",
  "field.lastUsed": "Son kullanım",
  "field.nextCharge": "Sonraki tahsilat",
  "field.payout": "Hakediş",
  "field.placed": "Verildi",
  "field.priceType": "Fiyat türü",
  "field.providers": "Sağlayıcılar",
  "field.quantity": "Adet",
  "field.reference": "Referans",
  "field.referenceId": "Referans kimliği",
  "field.region": "Bölge",
  "field.return": "İade",
  "field.run": "Çalışma",
  "field.scope": "Kapsam",
  "field.since": "Başlangıç",
  "field.started": "Başladı",
  "field.step": "Adım",
  "field.transactionKey": "İşlem anahtarı",
  "field.value": "Değer",
  "field.when": "Zaman",
  "field.number": "Numara",
  "field.currency": "Para birimi",
  "field.version": "Sürüm",
  "field.order": "Sipariş",
  "field.payment": "Ödeme",
  "field.fulfilment": "Gönderim",
  "field.draft": "Taslak",
  "field.canceled": "İptal edildi",
  "field.completed": "Tamamlandı",
  "field.basket": "Sepet grubu",
  "field.paymentCollection": "Ödeme kaydı",

  "detail.order.whereItStands": "Durumu",
  "detail.order.whereItStandsWhy":
    "Birbirinden bağımsız üç durum — siparişin kendisi, parası ve kolileri — hiçbiri diğerinin içine katlanmaz.",
  "detail.order.whoFor": "Kimin için",
  "detail.order.attachedTo": "Neye bağlı",
  "detail.order.basketWhy":
    "Sepet grubu bir pazaryeri ödemesidir: birkaç satıcıya tek ödeme, her birine ayrı sipariş.",
  "detail.order.versionWhy":
    "Her düzenlemede sürüm artar; önceki sürümler siparişin o anki halini saklar.",
  "detail.order.entitlements": "Verilen haklar",
  "detail.order.entitlementsWhy":
    "Bu siparişin hangi hakkı verdiği ve o hakkın hâlâ geçerli olup olmadığı.",

  "field.status": "Durum",
  "field.customer": "Müşteri",
  "field.cycle": "Dönem",
  "field.nextBilling": "Sonraki tahsilat",
  "field.currentPeriod": "Bu dönem",
  "field.endsThisPeriod": "Bu dönem sonunda biter",
  "field.ended": "Bitti",
  "field.dunningAttempts": "Tahsilat denemeleri",
  "field.sellingPlan": "Satış planı",
  "field.code": "Kod",
  "field.kind": "Tür",
  "field.applied": "Uygulanan",
  "field.used": "Kullanıldı",
  "field.perCustomer": "Müşteri başına",
  "field.campaign": "Kampanya dönemi",

  "detail.subscription.billed": "Neyin tahsil edildiği",
  "detail.subscription.cycle": "Dönem",
  "detail.subscription.dunningWhy":
    "Sıfırın üstü, bir tahsilatın başarısız olduğu ve yeniden denendiği anlamına gelir — iptal edilmiş bir sözleşmeden farklıdır, onu durum söyler.",
  "detail.subscription.who": "Kim ve ne",

  "detail.basket.orders": "Siparişler",
  "detail.basket.ordersWhy":
    "Bir sepet grubu, her satıcı için bir siparişe dönüşür — ödeme tektir, gönderim değildir.",
  "detail.basket.carts": "Sepetler",
  "detail.basket.cartsWhy":
    "Satıcının ödeme akışındaki kendi ayağı, siparişe dönüşmeden önce.",
  "detail.basket.payment": "Ödeme",

  "detail.promotion.title": "Kampanya",
  "detail.promotion.left": "Ne kadar kaldı",
  "detail.promotion.leftWhy":
    "Ödemede değil, sipariş verilirken düşülür — yani bu, üzerine söz verilmiş olandır.",

  "field.name": "Ad",
  "field.rate": "Oran",
  "field.starts": "Başlangıç",
  "field.ends": "Bitiş",
  "field.rules": "Kurallar",
  "field.priced": "Fiyatlı",
  "field.offeredToShoppers": "Müşteriye sunulur",
  "field.forReturns": "İadeler için",
  "field.serviceZone": "Hizmet bölgesi",
  "field.shippingProfile": "Gönderi profili",
  "field.optionType": "Seçenek tipi",
  "field.tax": "Vergi",
  "field.prices": "Fiyatlar",
  "field.workedOut": "Hesaplayan",
  "field.allowed": "İzin verilen",
  "field.defaultForRegion": "Bölgenin varsayılanı",
  "field.combinable": "Üstüne eklenebilir",
  "field.taxRegion": "Vergi bölgesi",
  "field.country": "Ülke",
  "field.province": "İl",
  "field.parentRegion": "Üst bölge",
  "field.provider": "Sağlayıcı",
  "field.amount": "Tutar",
  "field.captured": "Tahsil edildi",
  "field.session": "Oturum",
  "field.now": "Şu an",
  "field.issuedWith": "Çıkarılırken",
  "field.expires": "Son kullanma",
  "field.disabled": "Kapatıldı",
  "field.issuedOnOrder": "Çıkaran sipariş",

  "detail.priceList.title": "Liste",
  "detail.priceList.why":
    "İndirim listesi fiyatı düşürür ve bunu belli eder; geçersiz kılma ise sessizce değiştirir.",
  "detail.priceList.when": "Ne zaman geçerli",
  "detail.shippingOption.title": "Seçenek",
  "detail.shippingOption.where": "Nerede geçerli",
  "detail.shippingOption.whereWhy":
    "Hizmet bölgesi bu seçeneğin sunulduğu adresler kümesidir; profil ise hangi malları taşıyabileceğine karar verir.",
  "detail.region.title": "Bölge",
  "detail.region.taxWhy":
    "Gösterilen fiyatın vergiyi içerip içermediği ve vergiyi kimin hesapladığı.",
  "detail.region.providers": "Ödeme sağlayıcıları",
  "detail.taxRate.title": "Oran",
  "detail.taxRate.why":
    "Bir bölgenin tam olarak bir varsayılan oranı olur; eklenebilir bir oran, geçerli olanın üstüne biner.",
  "detail.taxRegion.where": "Nerede geçerli",
  "detail.taxRegion.whereWhy":
    "Vergi bölgeleri iç içedir: bir ilin oranları ülkesinin altında durur.",
  "detail.taxRegion.who": "Vergiyi kim hesaplıyor",
  "detail.payment.what": "Paraya ne olduğu",
  "detail.payment.whatWhy":
    "Provizyon ile tahsilat burada ayrı işlerdir; var olan bir ödeme, henüz alınmış bir ödeme değildir.",
  "detail.payment.where": "Nereye bağlı",
  "detail.giftCard.balance": "Bakiye",
  "detail.giftCard.origin": "Nereden geldiği",

  "field.symbol": "Simge",
  "field.nativeSymbol": "Yerel simge",
  "field.exponent": "Ondalık basamak",
  "field.numericCode": "Sayısal kod",
  "field.role": "Yetki",
  "field.password": "Parola",
  "field.balance": "Bakiye",
  "field.usable": "Kullanılabilir",

  "form.currency.title": "Yeni para birimi",
  "form.currency.why":
    "Ondalık basamak, bu para biriminin kaç haneyle yazıldığıdır — bir yazım bilgisi, çarpan değil: burada hiçbir şey kuruş cinsinden tutulmaz.",
  "form.currency.nativeWhy":
    "Bu para biriminin kendi dilinde yazan birinin klavyeye yazdığı simge.",
  "form.currency.exponentWhy":
    "0 ile 4 arası. Çoğu için iki; alt birimi olmayan bir para için sıfır.",
  "form.currency.numericWhy":
    "ISO 4217'nin bu para için verdiği numara. İsteğe bağlı.",

  "form.attributes.title": "Gönderi ve nitelikler",
  "form.attributes.why":
    "Taşıyıcının bunun için fiyat verebilmesi ve gümrüğün geçirebilmesi için gerekenler.",
  "form.attributes.hsWhy": "Gümrüğün bu tür bir malı ne diye adlandırdığı.",
  "form.attributes.originWhy":
    "İki harf. Nereden gönderildiği değil, nerede üretildiği.",

  "form.promotion.title": "Kampanyayı düzenle",
  "form.promotion.automatic": "Kendiliğinden uygulanır",
  "form.promotion.automaticWhy":
    "Kendiliğinden uygulanan bir kampanyada kasada kod yazılmaz.",
  "form.promotion.usesTotal": "Toplam kullanım",
  "form.promotion.usesTotalWhy":
    "Boş bırakılırsa sınır yoktur. Ödemede değil, sipariş verilirken düşülür.",
  "form.promotion.usesPerCustomer": "Müşteri başına kullanım",
  "form.promotion.noLimit": "Boş bırakılırsa sınır yoktur.",

  "form.organisation.why":
    "Bu ürünün neye ait olduğu ve geldiği sistemde ne diye anıldığı.",
  "form.organisation.anId": "Bir kimlik. Boş bırakmak temizler.",
  "form.organisation.externalWhy": "Bu ürünün geldiği yerde ne diye anıldığı.",

  "form.operator.title": "Yeni kullanıcı",
  "form.operator.why":
    "Parola burada belirlenir ve sonrasında kimseye gösterilmez. Kendi parolasını seçmesini isterseniz davet gönderin — sunucunun posta ayarı varsa gönderir.",
  "form.operator.roleWhy":
    "Açılan ilk hesap, burada ne yazarsa yazsın sahiptir — tek hesabı ikincisini açamayan bir dükkân kendini dışarıda bırakmıştır.",
  "form.operator.passwordWhy":
    "En az on iki karakter. Kendisine başka bir yoldan söyleyin — buradan parola gönderilmez.",

  "attached.storeCredit": "Mağaza bakiyesi",
  "attached.storeCreditWhy":
    "Dükkânın müşteriye borçlu olduğu bakiye; kasada karttan önce buradan düşülür.",
  "attached.taxNumbers": "Vergi numaraları",
  "attached.taxNumbersWhy":
    "Belirleyici olan doğrulanmış olmasıdır — doğrulanmamış bir numara, birinin yazdığı bir metindir.",
  "attached.exemptions": "Muafiyetler",
  "attached.exemptionsWhy":
    "Verginin alınmasını durduran belge; tek bir yerde ve iki tarih arasında geçerlidir.",

  "field.state": "Durum",
  "field.failure": "Hata",
  "field.currencyCode": "Para birimi kodu",
  "field.pricesIncludeTax": "Fiyatlara vergi dahil",
  "field.autoTax": "Vergiyi kendiliğinden hesapla",

  "detail.workflow.steps": "Adımlar",
  "detail.workflow.stepsWhy":
    "Her adım kendini nasıl geri alacağını bildirir; böylece geç bir hata, kendinden öncekilerin hepsini geri sararak yürür.",
  "detail.workflow.run": "Çalışma",
  "detail.channel.title": "Kanal",
  "detail.shippingProfile.title": "Profil",
  "detail.shippingProfile.why":
    "Bir gönderi seçeneğinin neyi taşıyabileceği — birlikte yolculuk eden mallar ve edemeyenler.",

  "form.region.new": "Yeni bölge",
  "form.region.edit": "Bölgeyi düzenle",
  "form.region.why":
    "Bölge, tek para birimiyle satış yapılan ve vergi konusunda tek bir cevabı olan ülkeler kümesidir.",
  "form.region.currencyWhy":
    "Mağazanın para birimlerinden biri. İki para birimiyle satan bir dükkân çevirmek yerine ikisinde de fiyat verir.",
  "form.region.taxWhy":
    "Buradaki müşteriye gösterilen: vergisi içinde olan bir fiyat mı, kasada vergi eklenen bir fiyat mı.",
  "form.product.new": "Yeni ürün",
  "form.product.newWhy":
    "Taslak olarak başlar. Varyantlar, fiyatlar ve stok ayrıca girilir.",

  "field.locale": "Dil",
  "field.disabled2": "Kapalı",

  "form.translations.title": "Çeviriler",
  "form.translations.why":
    "Bu ürünün başka bir dilde ne diye anıldığı. Mağaza bir dil ister, yoksa dükkânın kendi sözcüklerine döner.",
  "form.customer.edit": "Müşteriyi düzenle",
  "form.product.edit": "Ürünü düzenle",
  "form.channel.new": "Yeni satış kanalı",
  "form.channel.edit": "Satış kanalını düzenle",
  "form.channel.why":
    "Ürünün nerede satıldığı: bir web mağazası, bir uygulama, bir tezgâh. Bir ürün bunların bazılarına aittir, bazılarına değil.",
  "form.channel.disabledWhy":
    "Kapatılan bir kanal ürünlerini tutar, satmayı bırakır.",
  "form.key.title": "Yayımlanabilir anahtar üret",
  "form.key.why":
    "Anahtar, bir mağazayı okuyabileceği satış kanallarına bağlar. Belirteç yalnızca bir kez, hemen bundan sonra gösterilir.",
  "form.key.copyNow": "Bu anahtarı şimdi kopyalayın",
  "form.key.copyNowWhy":
    "Bir daha gösterilmeyecek. Kaybederseniz yenisini üretmek gerekir.",

  "batch.title": "İçe ve dışa aktarma",
  "batch.why":
    "Bir sayfa varyant CSV olarak dışarı, düzenlenip geri. İki yönde de aynı sütunlar.",
  "batch.export": "Dışa aktar",
  "batch.exportWhy":
    "Tek sayfa varyant, düz. Fiyatın fiyat olabilmesi için para birimi gerekir; para birimi belirtmeyen bir dışa aktarma o iki sütunu boş bırakır.",
  "batch.exported": "Dışa aktarılan satırlar, CSV olarak",
  "batch.import": "İçe aktar",
  "batch.importWhy":
    "Her satır bir varyanttır. Var olan bir kısa ad güncellenir, olmayan oluşturulur. Burada hiçbir şey silinmez — silmek kimlik ister ve bir hesap tablosu kimlik yazmanın yeri değildir.",

  "overview.title": "Genel bakış",
  "overview.why":
    "tezgah'ın yönetim yüzeyi ve bu panelin ne kadarını kapsadığı.",
  "overview.host": "Sunucu",
  "overview.hostWhy":
    "tezgah bir kütüphanedir. api::routes() işlevini başka bir şeyin bağlaması gerekir.",
  "overview.coverage": "Kapsam",

  "layout.workflows.why":
    "Çalıştırıcının yürüttüğü her koşu ve tamamlayamadığı her adım.",
  "layout.tax.why":
    "Nerede ne alındığı ve dükkânın kendisinin neye kayıtlı olduğu.",
  "layout.store.why": "Dükkânın nerede ve ne üzerinden sattığı.",
  "layout.pricing.why":
    "Listeler, tek bir tercih ve bir fiyatın arkasındaki kümeler ile satırlar.",
  "layout.payouts.why":
    "Bir satıcıya ne borçlu olunduğu ve bunu belirleyen komisyon.",
  "layout.payments.why":
    "Bir siparişten ne alındığı ve neden iade edilebileceği.",
  "layout.fulfilment.why":
    "Kimin taşıdığı, neyle gönderildiği ve dükkânın göndermek için ne aldığı.",

  "table.chooseEvery": "Bu sayfadaki her satırı seç",
  "table.chooseThis": "Bu satırı seç",
  "actions.menu": "İşlemler",

  "field.priceSetId": "Fiyat kümesi kimliği",
  "field.orderId": "Sipariş kimliği",

  "section.media": "Görsel",
  "section.mediaWhy":
    "Bir adres — bu dükkân dosya saklıyorsa buradan yüklenir, yoksa hâlihazırda sunulduğu yerden verilir.",
  "section.executions": "Çalışmalar",
  "section.executionsWhy": "Çalıştırıcının yürüttüğü her iş akışı koşusu.",
  "section.taxRules": "Neye uygulanır",
  "section.taxRulesWhy":
    "Kural, oranı tek bir tür şeye daraltır. Hiç kural yoksa bölgesindeki her şeye uygulanır.",
  "section.variants": "Varyantlar",
  "section.variantsWhy":
    "Fiyatı ve sayısı olan şey. Varyantı olmayan bir ürün satın alınamaz.",
  "section.levels": "Nerede ne var",
  "section.levelsWhy":
    "Her konum için ayrı sayılır. Sayımı yazıp hepsini birlikte kaydedin — tek çağrı, yani bir raf ya baştan sona sayılır ya hiç.",
  "section.movements": "Neler oldu",
  "section.movementsWhy":
    "Her ekleme ve her harcama. Bakiye bunların toplamıdır, birinin yazdığı bir sayı değil.",

  "screen.subscriptions.subtitle":
    "Sipariş değil, sözleşme. Ürettiği siparişler Siparişler altında listelenir.",
  "screen.records.subtitle":
    "Denetim kaydı ve giden kutusu, en yeniden başlayarak.",
  "screen.operators.subtitle":
    "Hesap bir kişiye aittir ve geri alınabilir. Yönetici anahtarı kimseye ait değildir ve geri alınamaz.",
  "screen.baskets.subtitle":
    "Sepet grupları kimlikle bulunur; kütüphanede listesi yoktur.",
}
