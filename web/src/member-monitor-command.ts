import { ApiFailure, type RequestScope } from "./api";
import { isMonitor, isMonitorPage, type MonitorRequest } from "./MemberWorkMonitors";
import { resolveStopTarget, toMonitorRequest, type MonitorIntent } from "./MemberWorkMonitorsChat";

export type MonitorCommand =
  | { kind: "list" }
  | { kind: "create"; body: MonitorRequest }
  | { kind: "stop"; target: string; resolved?: { id: string; revision: number } };

export function monitorCommand(intent: MonitorIntent, id: string, zone: string): MonitorCommand {
  if (intent.kind === "stop" || intent.kind === "list") return intent;
  return { kind: "create", body: toMonitorRequest(intent, id, zone) };
}

/** Bind the target before the write so retry cannot select another active watch. */
export async function submitMonitor(
  scope: Pick<RequestScope, "request">, command: MonitorCommand, thread: string, id: string, text: string,
  remember: (command: MonitorCommand) => void,
): Promise<string | null> {
  const path = "/personal/tasks/monitors";
  if (command.kind === "list") return null;
  if (command.kind === "create") {
    const receipt = await scope.request<unknown>(path, "POST", { ...command.body, thread_id: thread, message: text });
    if (!isMonitor(receipt) || receipt.id !== id) throw new ApiFailure(502, "monitor_invalid");
    return null;
  }
  let target = command.resolved;
  if (!target) {
    const page = await scope.request<unknown>(path);
    if (!isMonitorPage(page)) throw new ApiFailure(502, "monitor_invalid");
    const match = resolveStopTarget(command.target, page.items);
    if (match.found !== "one") return match.found === "many"
      ? "어느 감시를 멈출까요? 아래 목록에서 선택해 주세요." : "진행 중인 해당 감시를 찾지 못했습니다.";
    target = { id: match.id, revision: match.revision };
    remember({ ...command, resolved: target });
  }
  const receipt = await scope.request<unknown>(path + "/" + encodeURIComponent(target.id) + "/stop", "POST",
    { request_id: id, revision: target.revision });
  if (!isMonitor(receipt) || receipt.id !== target.id || receipt.state !== "stopped") throw new ApiFailure(502, "monitor_invalid");
  return null;
}
