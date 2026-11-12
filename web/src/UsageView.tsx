import { useEffect, useState } from "react";
import type { AccountUsageResponse, UsageSummary } from "../shared/api-types";
import { ApiFailure, isAborted } from "./api";
import { useScope } from "./account-scope";
import "./chat.css";

function valid(value: unknown): value is UsageSummary {
  return (
    !!value &&
    typeof value === "object" &&
    [
      "dispatched_calls",
      "unreported_calls",
      "reported_input_tokens",
      "reported_output_tokens",
      "reported_cache_read_tokens",
      "reported_cache_write_tokens",
    ].every((key) => {
      const count = (value as Record<string, unknown>)[key];
      return (
        typeof count === "number" && Number.isSafeInteger(count) && count >= 0
      );
    })
  );
}
function Summary({ value }: { value: UsageSummary }) {
  return (
    <>
      <div className="usage-stats">
        <div>
          <span>요청 횟수</span>
          <strong>{value.dispatched_calls.toLocaleString()}회</strong>
        </div>
        <div>
          <span>보고된 입력 토큰</span>
          <strong>{value.reported_input_tokens.toLocaleString()}</strong>
        </div>
        <div>
          <span>보고된 출력 토큰</span>
          <strong>{value.reported_output_tokens.toLocaleString()}</strong>
        </div>
      </div>
      <p className="usage-unknown">
        사용량 미보고 {value.unreported_calls.toLocaleString()}회
      </p>
      <p className="fine">
        위 토큰은 제공업체가 보고한 값의 합계입니다. 미보고 요청은 0토큰이나
        무료 요청을 뜻하지 않습니다. 재시도와 취소 요청에도 비용이 발생할 수
        있습니다.
      </p>
      <p className="fine">
        보고된 캐시 읽기 {value.reported_cache_read_tokens.toLocaleString()} ·
        캐시 쓰기 {value.reported_cache_write_tokens.toLocaleString()}
      </p>
    </>
  );
}
export function UsageView({
  accountId,
  admin = false,
  onExpired,
}: {
  accountId: string;
  admin?: boolean;
  onExpired: () => void;
}) {
  const scope = useScope(accountId, admin ? "admin-usage" : "personal-usage");
  const [summary, setSummary] = useState<UsageSummary | null>(null);
  const [accounts, setAccounts] = useState<AccountUsageResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError("");
    void scope
      .request<unknown>(
        admin ? "/admin/usage" : "/usage",
        "GET",
        undefined,
        controller.signal,
      )
      .then((value) => {
        if (admin) {
          const data = value as AccountUsageResponse;
          if (
            !data ||
            !Array.isArray(data.accounts) ||
            !data.accounts.every(
              (account) =>
                typeof account.account_id === "string" &&
                typeof account.name === "string" &&
                typeof account.email === "string" &&
                valid(account.usage),
            )
          )
            throw new Error("invalid_usage");
          setAccounts(data);
        } else {
          if (!valid(value)) throw new Error("invalid_usage");
          setSummary(value);
        }
        setLoading(false);
      })
      .catch((error) => {
        if (!isAborted(error)) {
          if (error instanceof ApiFailure && error.status === 401) onExpired();
          else setError("사용량을 확인하지 못했습니다. 다시 확인해 주세요.");
          setLoading(false);
        }
      });
    return () => controller.abort();
  }, [scope, admin, refresh]);
  return (
    <main className="admin-scroll usage-view">
      <div className="admin-intro">
        <p className="eyebrow">보고된 사용량</p>
        <h2>{admin ? "계정별 사용량" : "내 사용량"}</h2>
        <p>
          {admin
            ? "계정별 집계만 표시합니다. 개인 대화 내용은 표시하지 않습니다."
            : "내 계정에서 실행한 모델 요청과 보고된 토큰을 확인하세요."}
        </p>
      </div>
      {error && (
        <p className="notice" role="alert">
          {error}
        </p>
      )}
      {loading ? (
        <p role="status">사용량을 불러오는 중…</p>
      ) : admin && accounts ? (
        <section className="admin-card usage-table-wrap">
          <table className="usage-table">
            <caption>계정별 누적 사용량 · 보고된 토큰과 미보고 요청</caption>
            <thead>
              <tr>
                <th scope="col">계정</th>
                <th scope="col">요청</th>
                <th scope="col">보고된 입력</th>
                <th scope="col">보고된 출력</th>
                <th scope="col">미보고</th>
              </tr>
            </thead>
            <tbody>
              {accounts.accounts.map((account) => (
                <tr key={account.account_id}>
                  <th scope="row">
                    {account.name}
                    <small>{account.email}</small>
                  </th>
                  <td>{account.usage.dispatched_calls.toLocaleString()}</td>
                  <td>
                    {account.usage.reported_input_tokens.toLocaleString()}
                  </td>
                  <td>
                    {account.usage.reported_output_tokens.toLocaleString()}
                  </td>
                  <td>{account.usage.unreported_calls.toLocaleString()}회</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="fine">
            미보고 요청은 사용량을 알 수 없는 요청입니다. 0토큰이나 비용
            없음으로 계산하지 않습니다.
          </p>
        </section>
      ) : summary ? (
        <section className="admin-card">
          <Summary value={summary} />
        </section>
      ) : null}
      <button
        className="text-button"
        disabled={loading}
        onClick={() => setRefresh((value) => value + 1)}
      >
        사용량 새로고침
      </button>
    </main>
  );
}
