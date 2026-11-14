import { Component, Suspense, type ReactNode } from "react";

// Only public screen code is deferred. Account state, drafts and navigation live
// above this boundary and keep their existing authenticated lifetime.
export class DeferredPage extends Component<
  { children: ReactNode; back: () => void },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    if (this.state.failed) return (
      <section className="admin-scroll" aria-label="화면 연결 오류">
        <p role="alert">이 화면을 불러오지 못했어요. 연결을 확인한 뒤 앱을 새로고침해 주세요.</p>
        <button className="secondary" onClick={this.props.back}>채팅으로 돌아가기</button>{" "}
        <button className="primary" onClick={() => window.location.reload()}>앱 새로고침</button>
      </section>
    );
    return <Suspense fallback={<p className="admin-scroll" role="status">화면을 여는 중…</p>}>
      {this.props.children}
    </Suspense>;
  }
}
