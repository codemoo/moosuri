import { useEffect, useRef, useState } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import type { PushConfiguration, PushDeviceView } from "../shared/api-types";
import {
  beginPushEnrollment,
  completePushEnrollment,
  isPushEnrollmentCurrent,
  type PushBinding,
} from "./push-state";
import {
  activatePushAccount,
  deactivateBrowserPushBinding,
} from "./push-binding";
import { supportsPushBrowserLock, withPushBrowserLock } from "./push-lock";
import "./push.css";
const supported = () =>
  window.isSecureContext &&
  "Notification" in window &&
  "PushManager" in window &&
  "serviceWorker" in navigator &&
  "indexedDB" in window &&
  supportsPushBrowserLock();
function bytes(value: string) {
  const raw = atob(value.replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}
class StaleEnrollment extends Error {}
type Pending = {
  operation: PushBinding;
  body: {
    request_id: string;
    device_id: string;
    operation_revision: number;
    subscription: { endpoint: string; p256dh: string; auth: string };
  };
};
type PendingOff = {
  operation: PushBinding;
  body: { request_id: string; device_id: string; operation_revision: number };
};
const explanation = (device: PushDeviceView | undefined | null) =>
  device?.state === "active"
    ? device.last_outcome === "accepted"
      ? "최근 소식이 푸시 서비스에 접수되었습니다. 기기 표시·읽음은 확인할 수 없습니다."
      : device.last_outcome === "unavailable"
        ? "푸시 연결을 완료하지 못했습니다. 소식함은 계속 사용할 수 있습니다."
        : device.last_outcome === "queued"
          ? "푸시 서비스가 잠시 기다리도록 요청했습니다. 소식함에서는 바로 확인할 수 있습니다."
          : device.last_outcome === "unknown"
            ? "최근 전달 여부를 확인할 수 없습니다. 같은 소식을 자동으로 다시 보내지 않습니다."
            : "새 소식을 이 기기에도 알립니다."
    : device?.state === "off" && device.last_outcome === "off"
      ? "이 기기 알림은 꺼져 있습니다."
      : device?.state === "restore_required"
        ? "복원 후 다시 연결해야 합니다."
        : device
          ? "로그인 또는 연결 상태가 바뀌었습니다. 이 기기 알림을 다시 연결하세요."
          : "이 기기 알림은 꺼져 있습니다.";
type Props = { accountId: string; onExpired: () => void };
export function PushSettings(props: Props) {
  return <Contents key={props.accountId} {...props} />;
}
function Contents({ accountId, onExpired }: Props) {
  const scope = useScope(accountId, "push-settings");
  const [configuration, setConfiguration] = useState<PushConfiguration>();
  const [permission, setPermission] = useState<NotificationPermission>(
    typeof Notification === "undefined" ? "default" : Notification.permission,
  );
  const [online, setOnline] = useState(navigator.onLine);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [local, setLocal] = useState<PushBinding>();
  const [pending, setPending] = useState<Pending>();
  const [pendingOff, setPendingOff] = useState<PendingOff>();
  const lock = useRef(false);
  const alive = useRef(true);
  const generation = useRef(0);
  const capable = supported();
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      generation.current++;
    };
  }, []);
  async function refresh() {
    if (!capable || lock.current) return;
    const op = ++generation.current;
    try {
      const local = await activatePushAccount(accountId);
      const value = await scope.request<PushConfiguration>(
        `/notifications/push?device_id=${local.device_id}`,
      );
      if (!alive.current || op !== generation.current) return;
      setLocal(local);
      setConfiguration(value);
      setPermission(Notification.permission);
      setError("");
    } catch (e) {
      if (e instanceof ApiFailure && e.status === 401) onExpired();
      else if (!isAborted(e) && alive.current && op === generation.current)
        setError(
          "기기 알림 설정을 불러오지 못했습니다. 소식함은 계속 사용할 수 있습니다.",
        );
    }
  }
  useEffect(() => {
    void refresh();
    const focus = () => {
      setOnline(navigator.onLine);
      void refresh();
    };
    const offline = () => setOnline(false);
    window.addEventListener("online", focus);
    window.addEventListener("offline", offline);
    window.addEventListener("focus", focus);
    return () => {
      window.removeEventListener("online", focus);
      window.removeEventListener("offline", offline);
      window.removeEventListener("focus", focus);
    };
  }, [scope, accountId, capable]);
  async function requireCurrent(operation: PushBinding, token: number) {
    if (
      !alive.current ||
      token !== generation.current ||
      !(await isPushEnrollmentCurrent(operation)) ||
      !alive.current ||
      token !== generation.current
    )
      throw new StaleEnrollment();
  }
  async function submit(value: Pending, token: number) {
    await requireCurrent(value.operation, token);
    const result = await scope.request<PushDeviceView>(
      "/notifications/push",
      "POST",
      value.body,
    );
    await requireCurrent(value.operation, token);
    if (
      result.state !== "active" ||
      !(await completePushEnrollment(value.operation, result.binding_id))
    )
      throw new StaleEnrollment();
    if (!alive.current || token !== generation.current)
      throw new StaleEnrollment();
    setPending(undefined);
    setPendingOff(undefined);
    setConfiguration((old) => (old ? { ...old, device: result } : old));
    setLocal({ ...value.operation, binding_id: result.binding_id });
    setError("");
  }
  async function enable() {
    if (lock.current || !configuration || !capable || !navigator.onLine) return;
    lock.current = true;
    setBusy(true);
    setError("");
    const token = ++generation.current;
    let next: Pending | undefined;
    try {
      // Must run directly in the explicit click handler, before asynchronous IO.
      const permissionRequest =
        Notification.permission === "granted"
          ? Promise.resolve("granted" as NotificationPermission)
          : Notification.requestPermission();
      const [operation, answer] = await Promise.all([
        beginPushEnrollment(accountId),
        permissionRequest,
      ]);
      await requireCurrent(operation, token);
      setPermission(answer);
      if (answer !== "granted") return;
      const value = await withPushBrowserLock(async () => {
        // A queued newer intent must wait for any already-started unsubscribe.
        await requireCurrent(operation, token);
        const registration = await navigator.serviceWorker.getRegistration("/");
        await requireCurrent(operation, token);
        if (!registration) throw new Error("No service worker");
        const key = bytes(configuration.application_server_key);
        let subscription = await registration.pushManager.getSubscription();
        await requireCurrent(operation, token);
        const prior = subscription?.options.applicationServerKey;
        const same =
          prior &&
          new Uint8Array(prior as ArrayBuffer).length === key.length &&
          new Uint8Array(prior as ArrayBuffer).every((v, i) => v === key[i]);
        if (
          subscription &&
          (!same ||
            configuration.device?.last_outcome === "expired" ||
            configuration.device?.last_outcome === "rejected")
        ) {
          await requireCurrent(operation, token);
          await subscription.unsubscribe();
          await requireCurrent(operation, token);
          subscription = null;
        }
        if (!subscription) {
          await requireCurrent(operation, token);
          subscription = await registration.pushManager.subscribe({
            userVisibleOnly: true,
            applicationServerKey: key,
          });
        }
        await requireCurrent(operation, token);
        const value = subscription.toJSON();
        if (!value.endpoint || !value.keys?.p256dh || !value.keys.auth)
          throw new Error("Invalid subscription");
        return {
          endpoint: value.endpoint,
          p256dh: value.keys.p256dh,
          auth: value.keys.auth,
        };
      });
      next = {
        operation,
        body: {
          request_id: crypto.randomUUID(),
          device_id: operation.device_id,
          operation_revision: operation.revision,
          subscription: value,
        },
      };
      setPending(next);
      await submit(next, token);
    } catch (e) {
      if (e instanceof ApiFailure && e.status === 401) onExpired();
      else if (
        alive.current &&
        (e instanceof StaleEnrollment ||
          (e instanceof ApiFailure && e.status < 500))
      ) {
        setPending(undefined);
        setError(
          e instanceof ApiFailure && e.status === 400
            ? "이 브라우저의 푸시 연결을 지원하지 않거나 입력을 확인하지 못했습니다. 지원 브라우저와 연결 상태를 확인하세요."
            : "다른 탭이나 로그인에서 연결 상태가 바뀌었습니다. 상태를 확인한 뒤 다시 연결하세요.",
        );
      } else if (!isAborted(e) && alive.current)
        setError(
          next
            ? "연결 접수를 확인하지 못했습니다. 같은 연결 요청을 다시 확인할 수 있습니다."
            : "이 기기에서 알림을 연결하지 못했습니다. HTTPS 설치·브라우저 권한·네트워크를 확인하세요.",
        );
    } finally {
      lock.current = false;
      if (alive.current) setBusy(false);
    }
  }
  async function retry() {
    if (lock.current || !pending) return;
    lock.current = true;
    setBusy(true);
    const token = ++generation.current;
    try {
      await submit(pending, token);
    } catch (e) {
      if (e instanceof ApiFailure && e.status === 401) onExpired();
      else if (
        alive.current &&
        (e instanceof StaleEnrollment ||
          (e instanceof ApiFailure && e.status < 500))
      ) {
        setPending(undefined);
        setError(
          "다른 탭이나 로그인에서 연결 상태가 바뀌었습니다. 상태를 확인한 뒤 다시 연결하세요.",
        );
      } else if (!isAborted(e) && alive.current)
        setError(
          "같은 연결 요청을 확인하지 못했습니다. 네트워크를 확인하세요.",
        );
    } finally {
      lock.current = false;
      if (alive.current) setBusy(false);
    }
  }
  async function disable() {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    const token = ++generation.current;
    try {
      const value =
        pendingOff && (await isPushEnrollmentCurrent(pendingOff.operation))
          ? pendingOff
          : await (async () => {
              const operation = await deactivateBrowserPushBinding(accountId);
              return {
                operation,
                body: {
                  request_id: crypto.randomUUID(),
                  device_id: operation.device_id,
                  operation_revision: operation.revision,
                },
              };
            })();
      await requireCurrent(value.operation, token);
      setPendingOff(value);
      setLocal(value.operation);
      setPending(undefined);
      await scope.request("/notifications/push/disable", "POST", value.body);
      await requireCurrent(value.operation, token);
      setConfiguration((old) => (old ? { ...old, device: null } : old));
      setPendingOff(undefined);
      setError("");
    } catch (e) {
      if (e instanceof ApiFailure && e.status === 401) onExpired();
      else if (
        alive.current &&
        (e instanceof StaleEnrollment ||
          (e instanceof ApiFailure && e.status < 500))
      ) {
        setPendingOff(undefined);
        setError(
          "다른 탭이나 로그인에서 연결 상태가 바뀌었습니다. 상태를 확인한 뒤 다시 연결하세요.",
        );
      } else if (!isAborted(e) && alive.current)
        setError(
          "이 기기 표시는 껐지만 서버 연결 해제를 확인하지 못했습니다. 연결 후 다시 끄기를 눌러 확인하세요.",
        );
    } finally {
      lock.current = false;
      if (alive.current) setBusy(false);
    }
  }
  const shownDevice =
    configuration?.device?.state === "active" &&
    local?.binding_id !== configuration.device.binding_id
      ? { ...configuration.device, state: "reconnect" }
      : configuration?.device;
  return (
    <section className="push-settings" aria-label="기기 알림">
      <h2>이 기기 알림</h2>
      <p>
        잠금 화면에는 개인 내용 없이 새 소식 안내만 표시합니다. 누르면 로그인한
        계정의 소식함을 엽니다.
      </p>
      {!capable ? (
        <p>
          이 브라우저에서는 안전한 기기 알림 연결을 지원하지 않습니다. 최신 지원
          브라우저나 HTTPS 설치 앱에서 열어 주세요.
        </p>
      ) : (
        <>
          <p>
            {permission === "denied"
              ? "브라우저 알림이 차단되어 있습니다. 주소창·기기 설정에서 허용한 뒤 다시 확인하세요."
              : !online
                ? "현재 오프라인입니다. 연결 후 이 기기 알림을 설정하세요."
                : explanation(shownDevice)}
          </p>
          <div className="push-actions">
            <button
              type="button"
              disabled={
                busy || !configuration || permission === "denied" || !online
              }
              onClick={() => void enable()}
            >
              {busy
                ? "확인 중…"
                : shownDevice?.state === "active"
                  ? "이 기기 알림 다시 연결"
                  : "이 기기 알림 켜기"}
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void disable()}
            >
              이 기기 알림 끄기
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void refresh()}
            >
              연결 상태 확인
            </button>
            {pending && (
              <button
                type="button"
                disabled={busy || !online}
                onClick={() => void retry()}
              >
                같은 연결 요청 다시 확인
              </button>
            )}
          </div>
          <p className="fine">
            브라우저 허용 권한과 앱 로그인을 따로 유지합니다. 로그아웃·계정
            변경·조용한 시간에는 푸시를 표시하지 않으며 소식함에는 남습니다.
          </p>
        </>
      )}
      {error && <p role="status">{error}</p>}
    </section>
  );
}
