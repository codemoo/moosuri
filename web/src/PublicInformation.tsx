import type { ReactNode } from "react";

export type PublicInformationPage = "about" | "privacy" | "terms";

const pages: Record<string, PublicInformationPage> = {
  "/about": "about",
  "/privacy": "privacy",
  "/terms": "terms",
};

// Exact paths only; the gateway serves nothing else as a public information page.
export function resolvePublicInformationPath(pathname: string): PublicInformationPage | null {
  return Object.prototype.hasOwnProperty.call(pages, pathname) ? pages[pathname] : null;
}

const connections = "https://myaccount.google.com/connections";

function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return <section aria-labelledby={id}><h2 id={id}>{title}</h2>{children}</section>;
}

function About() {
  return <>
    <h1 id="info-title">Moosuri 소개</h1>
    <p className="info-lead">Moosuri는 직접 설치해 운영하는 개인 비서입니다. 회원은 자기 계정으로 대화하고, 일을 맡기고, 연결한 Google 계정의 정보를 확인합니다.</p>
    <p lang="en">Moosuri is a self-hosted personal assistant. Each member chats, delegates work and reviews information from their own connected accounts.</p>
    <Section id="about-what" title="할 수 있는 일">
      <ul>
        <li>대화와 맡긴 일, 기억은 회원별로 저장되고 다른 회원에게 보이지 않습니다.</li>
        <li>회원이 허용하면 Gmail 받은편지함의 최근 메일 제목·보낸 사람·날짜를 보여 줍니다. 메일 본문은 읽지 않습니다.</li>
        <li>Google Drive 파일 목록(이름·유형), Docs·Slides·Sheets의 제목과 구조 정보를 읽기 전용으로 확인합니다.</li>
        <li>일정을 별도로 연결하면 기본 캘린더를 조회하고, 회원이 최종 내용을 승인한 일정의 생성·시간 변경을 처리합니다.</li>
        <li>메일 발송은 조회 권한과 별도로 연결하며, 보낼 내용과 받는 사람을 회원이 승인한 뒤 진행합니다.</li>
        <li>비밀번호와 같은 로그인 정보는 별도 출처의 암호화 볼트에 보관합니다.</li>
      </ul>
    </Section>
    <Section id="about-google" title="Google 연결">
      <p>Google 연결은 회원이 직접 선택하고 Google 동의 화면에서 승인할 때만 만들어집니다. 이 설치는 OAuth 앱 하나를 공용으로 쓰지만, 승인과 토큰, 조회 결과는 회원마다 따로 보관됩니다.</p>
      <p>Gmail과 파일 조회는 읽기 전용 권한을 사용합니다. 메일 발송과 Calendar 일정 변경은 각각 별도 권한과 최종 승인이 필요합니다. 자세한 처리 방식은 <a href="/privacy">개인정보 처리방침</a>과 <a href="/terms">이용 약관</a>을 확인하세요.</p>
    </Section>
    <Section id="about-host" title="운영 방식">
      <p>Moosuri는 서버를 설치한 운영자가 직접 호스팅합니다. 서비스 제공자가 대신 운영하는 서비스가 아닙니다. 초대를 받은 회원은 초대한 설치 운영자에게 문의하세요.</p>
    </Section>
  </>;
}

function Privacy() {
  return <>
    <h1 id="info-title">개인정보 처리방침</h1>
    <p className="info-lead">이 문서는 Moosuri 설치본이 어떤 정보를 어떻게 다루는지 설명합니다. 서버는 설치 운영자가 운영하며, 아래 내용은 소프트웨어가 제공하는 동작을 기준으로 합니다.</p>
    <p lang="en">This explains how an installed Moosuri server handles information. The server is run by your installation operator.</p>
    <Section id="privacy-data" title="저장하는 정보">
      <ul>
        <li>가입 이메일은 로그인과 회원 관리에 사용합니다. 대화, 맡긴 일, 기억과 같은 작업 기록은 회원별 저장 공간에 보관됩니다.</li>
        <li>Google을 연결하면 연결한 계정 이메일, 허용한 서비스 권한, Google 토큰, 조회 결과와 기록이 회원의 개인 볼륨에 암호화되어 저장됩니다.</li>
        <li>Gmail은 현재 최근 메일의 제목·보낸 사람·날짜만 조회합니다. Drive는 파일 목록, 이름, 유형을, Docs·Slides·Sheets는 제목과 구조 정보를 읽기 전용으로 조회합니다.</li>
        <li>Calendar를 별도로 연결하면 기본 캘린더의 일정 제목·시각 등 일정 정보를 처리합니다. 일정 생성·시간 변경에는 별도의 Calendar 권한과 회원의 최종 내용 승인이 필요합니다.</li>
        <li>메일 발송을 별도로 연결하면 회원이 작성·승인한 초안, 받는 사람과 발송 결과를 처리하고 기록합니다. 받은편지함 조회 동의만으로 메일을 보내지 않습니다.</li>
        <li>조회한 결과는 같은 대화를 다시 열 때 보여 주기 위해 저장됩니다.</li>
      </ul>
    </Section>
    <Section id="privacy-google" title="Google 사용자 데이터">
      <p>Google 데이터는 회원이 요청한 조회와 승인한 메일·일정 작업을 처리하기 위해 사용합니다. 설치의 OAuth 클라이언트는 공용이지만 Google 승인, 토큰과 데이터는 회원마다 분리됩니다. 한 회원의 Google 데이터는 다른 회원에게 제공되지 않습니다.</p>
      <p>Google 서비스 연결은 회원이 서비스별로 명시적으로 동의해야 합니다. 로그인 정보는 별도 출처의 볼트에서 선택한 항목만 새로 만든 격리된 Kiri 로그인 환경으로 전달됩니다.</p>
    </Section>
    <Section id="privacy-vault" title="볼트와 관리자">
      <p>볼트의 비밀은 회원의 비밀번호로 기기에서 암호화되며 서버에는 암호문이 저장됩니다. 앱 관리자 권한만으로는 개인 볼트를 열 수 없고, 볼트를 열 수 있는 관리자 복구 기능은 없습니다.</p>
      <p>다만 서버 운영자는 서버 호스트의 저장소를 관리할 수 있습니다. 실행 중인 프로세스까지 운영자에게서 숨기는 기능은 제공하지 않습니다.</p>
    </Section>
    <Section id="privacy-model" title="모델 제공업체">
      <p>대화 처리를 위한 요청은 설치 관리자가 선택한 모델 제공업체로 전송됩니다. 비밀번호 같은 비밀은 모델 요청에 넣지 않습니다. 현재 받은편지함 조회 경로는 메일 내용을 모델에 보내지 않지만, 그 밖에 대화에 입력한 내용은 모델 제공업체로 전달될 수 있습니다. 제공업체의 처리 방식은 해당 업체의 정책을 따릅니다.</p>
    </Section>
    <Section id="privacy-delete" title="연결 해제와 삭제">
      <p>연결을 해제하면 Moosuri에서의 이후 사용이 중단되고, 해당 연결의 저장된 받은편지함 조회 결과가 제거됩니다. 대화·작업 기록이나 백업 전체를 삭제하거나 Google 쪽 접근 권한을 철회하는 것은 아닙니다.</p>
      <p>Google 쪽 권한은 <a href={connections} rel="noopener noreferrer">Google 계정 연결 관리</a>에서 직접 철회하세요. 저장된 데이터와 백업의 삭제는 설치 운영자에게 요청하고, 삭제 범위와 처리 일정을 확인하세요.</p>
    </Section>
    <Section id="privacy-contact" title="문의">
      <p>데이터 처리와 계정에 관한 문의는 회원을 초대했거나 이 설치를 운영하는 사람에게 하세요.</p>
    </Section>
  </>;
}

function Terms() {
  return <>
    <h1 id="info-title">이용 약관</h1>
    <p className="info-lead">이 설치본은 운영자가 초대한 회원을 위한 개인 비서입니다. 본인의 계정과 이용 권한이 있는 자료를 연결해 사용하세요.</p>
    <p lang="en">This installation serves members invited by its operator. Connect only accounts and information you are authorized to use.</p>
    <Section id="terms-account" title="계정 사용">
      <p>초대를 받은 본인의 계정만 사용하세요. 로그인 정보를 다른 사람과 공유하지 마세요. 회원은 자신이 연결한 계정과 입력한 내용에 책임이 있습니다.</p>
    </Section>
    <Section id="terms-google" title="Google 동의와 철회">
      <p>Google 서비스는 회원이 동의한 범위에서만 사용합니다. 동의는 언제든 <a href={connections} rel="noopener noreferrer">Google 계정 연결 관리</a>에서 철회할 수 있으며, Moosuri의 연결 해제는 이후 사용을 중단합니다.</p>
    </Section>
    <Section id="terms-third" title="외부 서비스">
      <p>Google과 모델 제공업체 같은 외부 서비스는 각자의 약관과 정책을 따르며 중단되거나 바뀔 수 있습니다. 이 때문에 일부 기능이 제한되거나 결과가 달라질 수 있습니다.</p>
    </Section>
    <Section id="terms-operator" title="운영자의 책임">
      <p>서버 운영, 백업, 데이터 삭제 요청 처리와 문의 응대는 이 설치를 운영하는 사람이 맡습니다. 소프트웨어 제작자가 이 서버를 호스팅하지 않습니다.</p>
    </Section>
    <Section id="terms-use" title="사용상 주의">
      <p>비서의 답변은 틀릴 수 있으므로 중요한 결정 전에는 직접 확인하세요. 법을 어기거나 다른 사람의 계정과 정보를 침해하는 용도로 사용하지 마세요.</p>
    </Section>
  </>;
}

export function PublicInformation({ page }: { page: PublicInformationPage }) {
  return <div className="public-information">
    <header>
      <a className="info-brand" href="/about"><img alt="" src="/moosuri-icon-192.png" width="32" height="32" />Moosuri</a>
      <a href="/app">앱 열기 · 로그인</a>
    </header>
    <main aria-labelledby="info-title">
      {page === "about" ? <About /> : page === "privacy" ? <Privacy /> : <Terms />}
    </main>
    <footer>
      <nav aria-label="공개 정보">
        <a href="/about" aria-current={page === "about" ? "page" : undefined}>소개</a>
        <a href="/privacy" aria-current={page === "privacy" ? "page" : undefined}>개인정보 처리방침</a>
        <a href="/terms" aria-current={page === "terms" ? "page" : undefined}>이용 약관</a>
      </nav>
    </footer>
  </div>;
}
