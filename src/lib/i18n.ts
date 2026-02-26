import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

import enCommon from "../../public/locales/en/common.json";
import enAuth from "../../public/locales/en/auth.json";
import enData from "../../public/locales/en/data.json";
import enStrategy from "../../public/locales/en/strategy.json";
import enBacktest from "../../public/locales/en/backtest.json";
import enOptimization from "../../public/locales/en/optimization.json";
import enExport from "../../public/locales/en/export.json";

import esCommon from "../../public/locales/es/common.json";
import esAuth from "../../public/locales/es/auth.json";
import esData from "../../public/locales/es/data.json";
import esStrategy from "../../public/locales/es/strategy.json";
import esBacktest from "../../public/locales/es/backtest.json";
import esOptimization from "../../public/locales/es/optimization.json";
import esExport from "../../public/locales/es/export.json";

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: {
        common: enCommon,
        auth: enAuth,
        data: enData,
        strategy: enStrategy,
        backtest: enBacktest,
        optimization: enOptimization,
        export: enExport,
      },
      es: {
        common: esCommon,
        auth: esAuth,
        data: esData,
        strategy: esStrategy,
        backtest: esBacktest,
        optimization: esOptimization,
        export: esExport,
      },
    },
    fallbackLng: "en",
    defaultNS: "common",
    ns: ["common", "auth", "data", "strategy", "backtest", "optimization", "export"],
    interpolation: {
      escapeValue: false,
    },
    detection: {
      order: ["localStorage", "navigator"],
      lookupLocalStorage: "lbquant-lang",
      caches: ["localStorage"],
    },
  });

export default i18n;
