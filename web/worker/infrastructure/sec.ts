import type { CompanyResearch } from "../../src/features/research/contracts";
import {
  upstream,
  array,
  record,
  string,
  number,
  ProviderError,
} from "./upstream";

export function annualFacts(
  payload: unknown,
  tags: string[],
  unit: string,
): Map<string, { filed: string; value: number }> {
  const gaap = record(record(record(payload).facts)["us-gaap"]);
  const periods = new Map<string, { filed: string; value: number }>();
  for (const tag of tags) {
    for (const item of array(record(record(gaap[tag]).units)[unit]).map(
      record,
    )) {
      const start = string(item.start),
        end = string(item.end),
        filed = string(item.filed),
        value = number(item.val);
      const duration = Date.parse(end) - Date.parse(start);
      if (
        !start ||
        !end ||
        !filed ||
        !Number.isFinite(duration) ||
        duration < 330 * 86400000 ||
        duration > 380 * 86400000 ||
        value === null
      )
        continue;
      if (!periods.has(end) || periods.get(end)!.filed < filed)
        periods.set(end, { filed, value });
    }
  }
  return periods;
}

export async function getCompany(symbol: string): Promise<CompanyResearch> {
  const tickers = record(
    await upstream(
      "https://www.sec.gov/files/company_tickers.json",
      86400,
      2 * 1024 * 1024,
    ),
  );
  const ticker = Object.values(tickers)
    .map(record)
    .find((item) => string(item.ticker) === symbol);
  const cikNumber = ticker && number(ticker.cik_str);
  if (!cikNumber)
    throw new ProviderError(
      "SEC company research is available for US reporting companies. This instrument has no matched filing record.",
    );
  const cik = String(cikNumber).padStart(10, "0");
  const [facts, submissionsRaw] = await Promise.all([
    upstream(`https://data.sec.gov/api/xbrl/companyfacts/CIK${cik}.json`, 3600),
    upstream(
      `https://data.sec.gov/submissions/CIK${cik}.json`,
      3600,
      4 * 1024 * 1024,
    ),
  ]);
  const submissions = record(submissionsRaw),
    recent = record(record(submissions.filings).recent);
  const revenue = annualFacts(
    facts,
    [
      "Revenues",
      "RevenueFromContractWithCustomerExcludingAssessedTax",
      "SalesRevenueNet",
    ],
    "USD",
  );
  const operating = annualFacts(facts, ["OperatingIncomeLoss"], "USD");
  const net = annualFacts(facts, ["NetIncomeLoss", "ProfitLoss"], "USD");
  const eps = annualFacts(facts, ["EarningsPerShareDiluted"], "USD/shares");
  return {
    name: string(submissions.name),
    cik,
    industry: string(submissions.sicDescription),
    fiscalYearEnd: string(submissions.fiscalYearEnd),
    periods: [...revenue]
      .sort(([a], [b]) => b.localeCompare(a))
      .slice(0, 5)
      .map(([end, fact]) => ({
        end,
        filed: fact.filed,
        revenue: fact.value,
        operatingIncome: operating.get(end)?.value ?? null,
        netIncome: net.get(end)?.value ?? null,
        eps: eps.get(end)?.value ?? null,
      })),
    filings: array(recent.form)
      .slice(0, 12)
      .map((form, index) => {
        const accession = string(
          array(recent.accessionNumber)[index],
        ).replaceAll("-", "");
        const document = string(array(recent.primaryDocument)[index]);
        return {
          form: string(form),
          date: string(array(recent.filingDate)[index]),
          title:
            string(array(recent.primaryDocDescription)[index]) || string(form),
          url: `https://www.sec.gov/Archives/edgar/data/${cikNumber}/${encodeURIComponent(accession)}/${encodeURIComponent(document)}`,
        };
      }),
  };
}
