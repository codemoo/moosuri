import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, it, expect, vi } from "vitest";
import { useMediaUploads, PrivateImage } from "./Media";
import { RequestScope } from "./api";
import { prepareImage, imageDimensions } from "./media-prepare";
import type { MediaSummary } from "../shared/api-types";
vi.mock("./media-prepare", async (importOriginal) => {
  const original = await importOriginal<typeof import("./media-prepare")>();
  return { ...original, prepareImage: vi.fn() };
});
const summary: MediaSummary = {
  id: "11111111-1111-4111-8111-111111111111",
  revision: 1,
  state: "ready",
  digest: "synthetic",
  width: 32,
  height: 32,
  bytes: 123,
  created_at: 1,
};
class Upload {
  static jobs: Upload[] = [];
  upload: { onprogress: ((e: ProgressEvent) => void) | null } = {
    onprogress: null,
  };
  onload: (() => void) | null = null;
  onabort: (() => void) | null = null;
  onerror: (() => void) | null = null;
  status = 200;
  responseText = JSON.stringify(summary);
  withCredentials = false;
  headers: Record<string, string> = {};
  url = "";
  body?: Blob;
  constructor() {
    Upload.jobs.push(this);
  }
  open(_method: string, url: string) {
    this.url = url;
  }
  setRequestHeader(k: string, v: string) {
    this.headers[k] = v;
  }
  send(body: Blob) {
    this.body = body;
  }
  abort() {
    this.onabort?.();
  }
  finish() {
    this.onload?.();
  }
}
beforeEach(() => {
  Upload.jobs = [];
  vi.stubGlobal("XMLHttpRequest", Upload);
  vi.stubGlobal("fetch", vi.fn());
  vi.stubGlobal(
    "URL",
    Object.assign(URL, {
      createObjectURL: vi.fn(() => `blob:synthetic-${Math.random()}`),
      revokeObjectURL: vi.fn(),
    }),
  );
  vi.mocked(prepareImage).mockResolvedValue(
    new Blob(["prepared bytes"], { type: "image/jpeg" }),
  );
});
function Harness() {
  const media = useMediaUploads("account-a", "thread-a");
  const [text, setText] = __state("");
  return (
    <>
      {media.view}
      <input
        aria-label="text"
        value={text}
        onChange={(e) => setText(e.target.value)}
      />
      <button onClick={() => media.sent(media.readySelections)}>
        Send text: {media.ready.length} images
      </button>
      <output>{JSON.stringify(media.ready)}</output>
    </>
  );
}
import { useState as __state, useRef as __ref } from "react";
it("pending uploads do not block unrelated text; ready snapshots remove only their own selection", async () => {
  const view = render(<Harness />);
  fireEvent.change(screen.getByLabelText("이미지 첨부"), {
    target: { files: [new File(["raw"], "x.jpg")] },
  });
  await waitFor(() => expect(Upload.jobs).toHaveLength(1));
  fireEvent.change(screen.getByLabelText("text"), {
    target: { value: "unrelated text" },
  });
  fireEvent.click(screen.getByText("Send text: 0 images"));
  expect(screen.getByText(/업로드 0%/)).toBeInTheDocument();
  act(() => {
    Upload.jobs[0].upload.onprogress?.({
      lengthComputable: true,
      loaded: 50,
      total: 100,
    } as ProgressEvent);
  });
  expect(screen.getByText("업로드 50%")).toBeInTheDocument();
  act(() => Upload.jobs[0].finish());
  await screen.findByText("첨부 준비됨");
  expect(screen.getByText("Send text: 1 images")).toBeEnabled();
  fireEvent.click(screen.getByText("Send text: 1 images"));
  await waitFor(() =>
    expect(screen.queryByText("첨부 준비됨")).not.toBeInTheDocument(),
  );
  expect(URL.revokeObjectURL).toHaveBeenCalled();
  view.unmount();
});
it("lost upload ACK retries the same UUID and exact prepared Blob", async () => {
  render(<Harness />);
  fireEvent.change(screen.getByLabelText("이미지 첨부"), {
    target: { files: [new File(["raw"], "x.jpg")] },
  });
  await waitFor(() => expect(Upload.jobs).toHaveLength(1));
  act(() => Upload.jobs[0].onerror?.());
  fireEvent.click(await screen.findByText("같은 파일 재시도"));
  await waitFor(() => expect(Upload.jobs).toHaveLength(2));
  expect(Upload.jobs[1].url).toBe(Upload.jobs[0].url);
  expect(Upload.jobs[1].body).toBe(Upload.jobs[0].body);
  expect(Upload.jobs[1].headers["X-Moosuri-Account"]).toBe("account-a");
});
it("cancel posts a tombstone and account lifetime closes work and object URLs", async () => {
  vi.mocked(fetch).mockResolvedValue(new Response(null, { status: 204 }));
  const view = render(<Harness />);
  fireEvent.change(screen.getByLabelText("이미지 첨부"), {
    target: { files: [new File(["raw"], "x.jpg")] },
  });
  await waitFor(() => expect(Upload.jobs).toHaveLength(1));
  fireEvent.click(screen.getByText("취소"));
  await waitFor(() => expect(fetch).toHaveBeenCalled());
  expect(vi.mocked(fetch).mock.calls[0][1]?.method).toBe("DELETE");
  expect(URL.revokeObjectURL).toHaveBeenCalled();
  view.unmount();
  const scope = new RequestScope("account-a");
  scope.localUrl(new Blob(["synthetic"]));
  const controller = new AbortController();
  const upload = scope.upload("id", new Blob(), () => {}, controller.signal);
  scope.close();
  await expect(upload).rejects.toMatchObject({ name: "AbortError" });
  expect(URL.revokeObjectURL).toHaveBeenCalled();
});
it("an old account response cannot create a new private preview URL", async () => {
  let release!: (r: Response) => void;
  vi.mocked(fetch).mockImplementation(
    () => new Promise((resolve) => (release = resolve)),
  );
  const scope = new RequestScope("account-a");
  const pending = scope.imageUrl("/media/id/thumbnail");
  scope.close();
  release(
    new Response(new Blob(["private image"]), {
      headers: { "content-type": "image/jpeg" },
    }),
  );
  await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});
it("deletion revokes an already displayed authenticated thumbnail", async () => {
  vi.mocked(fetch).mockResolvedValue(
    new Response(new Blob(["private image"]), {
      headers: { "content-type": "image/jpeg" },
    }),
  );
  render(<PrivateImage accountId="account-a" media={summary} />);
  await screen.findByAltText("첨부 이미지");
  act(() =>
    window.dispatchEvent(
      new CustomEvent("moosuri-media-deleted", { detail: summary.id }),
    ),
  );
  expect(screen.queryByAltText("첨부 이미지")).not.toBeInTheDocument();
  expect(URL.revokeObjectURL).toHaveBeenCalled();
});
it("header preflight reads PNG dimensions and rejects active formats before decode", () => {
  const bytes = new Uint8Array(24);
  bytes.set([137, 80, 78, 71]);
  const v = new DataView(bytes.buffer);
  v.setUint32(16, 12000);
  v.setUint32(20, 12000);
  expect(imageDimensions(bytes)).toEqual([12000, 12000]);
  expect(() =>
    imageDimensions(new TextEncoder().encode("<svg onload=x>")),
  ).toThrow(/PNG/);
});

function SelectionRace() {
  const [slot, setSlot] = __state("a");
  const media = useMediaUploads("account-a", slot);
  const captured = __ref<string[]>([]);
  return (
    <>
      {media.view}
      <button onClick={() => media.select(summary)}>
        Select library image
      </button>
      <button
        onClick={() => {
          captured.current = [...media.readySelections];
        }}
      >
        Capture send
      </button>
      <button onClick={() => setSlot((v) => (v === "a" ? "b" : "a"))}>
        Switch draft
      </button>
      <button onClick={() => media.sent(captured.current)}>
        Acknowledge captured send
      </button>
      <output aria-label="ready count">{media.ready.length}</output>
    </>
  );
}
it("an older ACK preserves the same library image selected in another draft", () => {
  render(<SelectionRace />);
  fireEvent.click(screen.getByText("Select library image"));
  fireEvent.click(screen.getByText("Capture send"));
  fireEvent.click(screen.getByText("Switch draft"));
  fireEvent.click(screen.getByText("Select library image"));
  fireEvent.click(screen.getByText("Acknowledge captured send"));
  expect(screen.getByLabelText("ready count")).toHaveTextContent("1");
  fireEvent.click(screen.getByText("Switch draft"));
  expect(screen.getByLabelText("ready count")).toHaveTextContent("0");
});
it("an older ACK preserves a later deselect/reselect instance of the same image", () => {
  render(<SelectionRace />);
  fireEvent.click(screen.getByText("Select library image"));
  fireEvent.click(screen.getByText("Capture send"));
  fireEvent.click(screen.getByText("선택 해제"));
  fireEvent.click(screen.getByText("Select library image"));
  fireEvent.click(screen.getByText("Acknowledge captured send"));
  expect(screen.getByLabelText("ready count")).toHaveTextContent("1");
});
