import { useEffect, useState } from "react";
import type { AccountUsageResponse, UsageSummary, RunTimingAggregate, LatencySamples } from "../shared/api-types";
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
function latency(v: LatencySamples) {
  const time=(n:number|null)=>n===null?"측정값 없음":`${(n/1000).toFixed(2)}초`;
  return `중앙값 ${time(v.p50_ms)} · 95백분위 ${time(v.p95_ms)} · 표본 ${v.samples} · 누락 ${v.missing}`;
}
function Timings({value}:{value:RunTimingAggregate}) {
 return <section aria-label="서버 응답 시간"><h3>보관 중인 최근 서버 응답 시간</h3>
 <p>{value.runs}개 실행 · {value.attempts}개 접수/재시도{value.truncated?" · 최근 128개 실행으로 제한":""} · 과거 시간 정보 없음 {value.unmeasured_runs}개</p>
 <dl>{([["대기",value.queue],["문맥 준비",value.context],["첫 모델 호출까지",value.first_dispatch],["모델 응답 소요",value.model],["도구 수행",value.tool],["완료한 최종 답변의 첫 본문",value.first_final_body],["종료까지",value.terminal]] as const).map(([name,v])=><div key={name}><dt>{name}</dt><dd>{latency(v)}</dd></div>)}</dl>
 <p>완료 {value.outcomes.completed??0} · 실패 {value.outcomes.failed??0} · 취소 {value.outcomes.cancelled??0} · 결과 불명 {value.outcomes.outcome_unknown??0} · 일부 결과 {value.outcomes.incomplete??0} · 거절 {value.outcomes.refused??0} · 진행/대기 {(value.outcomes.running??0)+(value.outcomes.queued??0)} · 종료 상태 미관측 {value.outcomes.unobserved??0}</p>
 <p className="fine">서버에서 실제 관측한 시간입니다. 화면 표시·브라우저 왕복 시간은 측정하지 않습니다. 첫 모델 호출은 전송 의도를 저장한 시점이며 제공업체 수락을 뜻하지 않습니다. 해설·도구 결과·최종 단계가 없는 출력은 첫 최종 본문 표본으로 세지 않습니다. 누락은 0초가 아닙니다. 모델 호출 없이 끝났거나 대기 중인 접수 {value.not_dispatched}개.</p>
 </section>;
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
      {value.timings&&<Timings value={value.timings}/> }
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
                <th scope="col">최근 응답 시간</th>
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
                  <td>{account.usage.timings?<><p>첫 최종 본문: {latency(account.usage.timings.first_final_body)}</p><p>종료: {latency(account.usage.timings.terminal)}</p><p>{account.usage.timings.attempts}개 접수/재시도 · 시간 정보 없는 과거 실행 {account.usage.timings.unmeasured_runs}개</p></>:"측정값 없음"}</td>
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
