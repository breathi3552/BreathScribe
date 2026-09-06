import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const LOCALES_DIR = path.join(__dirname, "..", "src", "i18n", "locales");

const handyAckMap: Record<
  string,
  { title: string; description: string; details: string }
> = {
  en: {
    title: "Handy",
    description: "Open-source speech-to-text desktop application by @cjpais",
    details:
      "BreathScribe is a fork of Handy, extending it with cloud models and proxy capabilities. Huge thanks to CJ Pais and all contributors of the original Handy project.",
  },
  zh: {
    title: "Handy",
    description: "由 @cjpais 开源的桌面语音转文字应用",
    details:
      "BreathScribe 是 Handy 的衍生分支，扩展了云端大模型通道与网络代理等能力。特别感谢 CJ Pais 及原 Handy 项目的所有贡献者。",
  },
  "zh-TW": {
    title: "Handy",
    description: "由 @cjpais 開源的桌面語音轉文字應用程式",
    details:
      "BreathScribe 是 Handy 的衍生分支，擴展了雲端大模型通道與網路代理等能力。特別感謝 CJ Pais 及原 Handy 專案的所有貢獻者。",
  },
  ja: {
    title: "Handy",
    description:
      "@cjpais によるオープンソースのデスクトップ音声テキスト変換アプリ",
    details:
      "BreathScribe は Handy のフォークであり、クラウドモデルとプロキシ機能を追加拡張しています。CJ Pais 氏および元の Handy プロジェクトのすべての貢献者に心より感謝申し上げます。",
  },
  ko: {
    title: "Handy",
    description:
      "@cjpais가 개발한 오픈 소스 데스크톱 음성 텍스트 변환 애플리케이션",
    details:
      "BreathScribe는 Handy의 포크 버전으로, 클라우드 모델 및 프록시 기능을 확장했습니다. CJ Pais 님과 원래 Handy 프로젝트의 모든 기여자분들께 깊은 감사를 드립니다.",
  },
  es: {
    title: "Handy",
    description:
      "Aplicación de escritorio de voz a texto de código abierto por @cjpais",
    details:
      "BreathScribe es una bifurcación de Handy, que lo amplía con modelos en la nube y capacidades de proxy. Muchas gracias a CJ Pais y a todos los colaboradores del proyecto Handy original.",
  },
  fr: {
    title: "Handy",
    description:
      "Application bureautique de synthèse vocale open-source par @cjpais",
    details:
      "BreathScribe est un fork de Handy, l'enrichissant de modèles cloud et de fonctionnalités de proxy. Un immense merci à CJ Pais et à tous les contributeurs du projet Handy original.",
  },
  de: {
    title: "Handy",
    description: "Open-Source-Desktop-Sprache-zu-Text-Anwendung von @cjpais",
    details:
      "BreathScribe ist ein Fork von Handy und erweitert es um Cloud-Modelle und Proxy-Funktionen. Vielen Dank an CJ Pais und alle Mitwirkenden des ursprünglichen Handy-Projekts.",
  },
  ru: {
    title: "Handy",
    description:
      "Настольное приложение преобразования речи в текст с открытым исходным кодом от @cjpais",
    details:
      "BreathScribe — это форк Handy, расширенный облачными моделями и поддержкой прокси. Огромная благодарность CJ Pais и всем участникам оригинального проекта Handy.",
  },
  pt: {
    title: "Handy",
    description:
      "Aplicativo de voz para texto de código aberto para desktop por @cjpais",
    details:
      "BreathScribe é um fork do Handy, estendendo-o com modelos em nuvem e recursos de proxy. Muito obrigado a CJ Pais e a todos os colaboradores do projeto Handy original.",
  },
  it: {
    title: "Handy",
    description:
      "Applicazione desktop open-source di sintesi vocale da testo di @cjpais",
    details:
      "BreathScribe è un fork di Handy, arricchito con modelli cloud e funzionalità proxy. Un ringraziamento speciale a CJ Pais e a tutti i collaboratori del progetto originale Handy.",
  },
  nl: {
    title: "Handy",
    description: "Open-source spraak-naar-tekst desktopapplicatie door @cjpais",
    details:
      "BreathScribe is een fork van Handy, uitgebreid met cloudmodellen en proxymogelijkheden. Veel dank aan CJ Pais en alle bijdragers van het originele Handy-project.",
  },
  pl: {
    title: "Handy",
    description:
      "Aplikacja desktopowa zamiany mowy na tekst o otwartym kodzie źródłowym autorstwa @cjpais",
    details:
      "BreathScribe to fork projektu Handy, rozszerzony o modele chmurowe i obsługę proxy. Ogromne podziękowania dla CJ Paisa i wszystkich współtwórców oryginalnego projektu Handy.",
  },
  tr: {
    title: "Handy",
    description:
      "@cjpais tarafından geliştirilen açık kaynaklı masaüstü sesten metne dönüştürme uygulaması",
    details:
      "BreathScribe, bulut modelleri ve proxy yetenekleriyle genişletilmiş bir Handy çatalıdır. CJ Pais'e ve orijinal Handy projesinin tüm katkıda bulunanlarına çok teşekkürler.",
  },
  uk: {
    title: "Handy",
    description:
      "Програма для перетворення мови в текст із відкритим вихідним кодом від @cjpais",
    details:
      "BreathScribe є форком Handy, що розширює його хмарними моделями та підтримкою проксі. Велика подяка CJ Pais та всім авторам оригінального проєкту Handy.",
  },
  vi: {
    title: "Handy",
    description:
      "Ứng dụng chuyển giọng nói thành văn bản mã nguồn mở trên máy tính bởi @cjpais",
    details:
      "BreathScribe là một bản phân nhánh của Handy, mở rộng thêm các mô hình đám mây và khả năng proxy. Xin chân thành cảm ơn CJ Pais và tất cả những người đóng góp cho dự án Handy ban đầu.",
  },
  cs: {
    title: "Handy",
    description:
      "Open-source desktopová aplikace pro převod řeči na text od @cjpais",
    details:
      "BreathScribe je fork projektu Handy, rozšířený o cloudové modely a funkce proxy. Velké díky patří CJ Paisovi a všem přispěvatelům původního projektu Handy.",
  },
  da: {
    title: "Handy",
    description: "Open source tale-til-tekst skrivebordsapplikation af @cjpais",
    details:
      "BreathScribe er et fork af Handy, der udvider det med cloudmodeller og proxy-funktioner. Mange tak til CJ Pais og alle bidragydere til det oprindelige Handy-projekt.",
  },
  sv: {
    title: "Handy",
    description:
      "Skrivbordsapplikation för tal-till-text med öppen källkod av @cjpais",
    details:
      "BreathScribe är en förgrening av Handy, utökad med molnmodeller och proxyfunktioner. Stort tack till CJ Pais och alla bidragsgivare till det ursprungliga Handy-projektet.",
  },
  bg: {
    title: "Handy",
    description:
      "Настолно приложение за преобразуване на реч в текст с отворен код от @cjpais",
    details:
      "BreathScribe е разклонение на Handy, надградено с облачни модели и прокси възможности. Огромни благодарности на CJ Pais и всички сътрудници на оригиналния проект Handy.",
  },
  ar: {
    title: "Handy",
    description: "تطبيق مكتبي مفتوح المصدر لتحويل الكلام إلى نص بواسطة @cjpais",
    details:
      "BreathScribe هو فرع مشتق من Handy، تم توسيعه بنماذج سحابية وقدرات وكيل البروكسي. شكراً جزيلاً لـ CJ Pais وجميع المساهمين في مشروع Handy الأصلي.",
  },
  he: {
    title: "Handy",
    description:
      "אפליקציית שולחן עבודה בקוד פתוח להמרת דיבור לטקסט מאת @cjpais",
    details:
      "BreathScribe הוא פיצול של Handy, המרחיב אותו עם מודלים בענן ויכולות פרוקסי. תודה רבה ל-CJ Pais ולכל התורמים לפרויקט Handy המקורי.",
  },
  hi: {
    title: "Handy",
    description:
      "@cjpais द्वारा विकसित ओपन-सोर्स डेस्कटॉप स्पीच-टू-टेक्स्ट एप्लिकेशन",
    details:
      "BreathScribe Handy का एक फ़ॉर्क है, जिसे क्लाउड मॉडल और प्रॉक्सी क्षमताओं के साथ विस्तारित किया गया है। CJ Pais और मूल Handy प्रोजेक्ट के सभी योगदानकर्ताओं को बहुत-बहुत धन्यवाद।",
  },
  ne: {
    title: "Handy",
    description:
      "@cjpais द्वारा विकसित खुला स्रोत डेस्कटप स्पिच-टु-टेक्स्ट अनुप्रयोग",
    details:
      "BreathScribe Handy को एक फोर्क हो, जसलाई क्लाउड मोडेल र प्रोक्सी क्षमताहरूसँग विस्तार गरिएको छ। CJ Pais र मूल Handy परियोजनाका सबै योगदानकर्ताहरूलाई धेरै धेरै धन्यवाद।",
  },
};

function rebrandString(val: string): string {
  let res = val;
  // 1. Handy Cloud -> BreathScribe
  res = res.replace(/Handy\s+Cloud/g, "BreathScribe");
  // 2. HANDY_DISABLE_UPDATER -> BREATHSCRIBE_DISABLE_UPDATER
  res = res.replace(/HANDY_DISABLE_UPDATER/g, "BREATHSCRIBE_DISABLE_UPDATER");
  // 3. Isolated Handy (e.g. Handy-genveje, Handy가, ל-Handy, etc.)
  res = res.replace(/Handy/g, "BreathScribe");
  return res;
}

function processObject(obj: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(obj)) {
    if (typeof value === "string") {
      result[key] = rebrandString(value);
    } else if (
      typeof value === "object" &&
      value !== null &&
      !Array.isArray(value)
    ) {
      result[key] = processObject(value as Record<string, unknown>);
    } else {
      result[key] = value;
    }
  }
  return result;
}

function run() {
  const entries = fs.readdirSync(LOCALES_DIR, { withFileTypes: true });
  const langs = entries
    .filter((e) => e.isDirectory())
    .map((e) => e.name)
    .sort();

  console.log(`Found ${langs.length} languages to process.`);

  for (const lang of langs) {
    const filePath = path.join(LOCALES_DIR, lang, "translation.json");
    if (!fs.existsSync(filePath)) continue;

    const raw = fs.readFileSync(filePath, "utf8");
    const json = JSON.parse(raw) as Record<string, any>;

    // Process all strings
    const processed = processObject(json);

    // Ensure settings.about.acknowledgments.handy exists
    if (!processed.settings) processed.settings = {};
    if (!processed.settings.about) processed.settings.about = {};
    if (!processed.settings.about.acknowledgments)
      processed.settings.about.acknowledgments = {};

    const ack = handyAckMap[lang] || handyAckMap.en;
    processed.settings.about.acknowledgments.handy = {
      title: ack.title,
      description: ack.description,
      details: ack.details,
    };

    // Keep key order in acknowledgments: handy first, then ggml
    const oldGgml = processed.settings.about.acknowledgments.ggml;
    processed.settings.about.acknowledgments = {
      title: processed.settings.about.acknowledgments.title,
      handy: processed.settings.about.acknowledgments.handy,
      ggml: oldGgml,
    };

    fs.writeFileSync(
      filePath,
      JSON.stringify(processed, null, 2) + "\n",
      "utf8",
    );
    console.log(`✓ Processed ${lang}`);
  }

  console.log("All translation files successfully updated!");
}

run();
