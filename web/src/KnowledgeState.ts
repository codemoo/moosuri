import type {
  EvidenceRef,
  IngestBatch,
  KnowledgeCollection,
  MemoryDocument,
  MemoryDraft,
  SourceDocument,
} from "../shared/api-types";

export const bytes = (value: string) =>
  new TextEncoder().encode(value).byteLength;
export const emptyMemory = (): MemoryDraft => ({
  text: "",
  subject: "",
  speaker: null,
  valid_from: null,
  valid_until: null,
  evidence: [],
});
export type MemoryWrite = {
  request_id: string;
  draft: MemoryDraft;
  id: string | null;
  expected_revision: number | null;
};
export type MemoryEditorState = {
  open: boolean;
  id: string | null;
  expected_revision: number | null;
  draft: MemoryDraft;
  request: MemoryWrite | null;
  issue: "uncertain" | "conflict" | null;
};
export type PasteState = {
  open: boolean;
  collection: KnowledgeCollection;
  title: string;
  text: string;
  speaker: string;
  observed_at: number;
  valid_from: number | null;
  valid_until: number | null;
  request: IngestBatch | null;
  issue: "uncertain" | "conflict" | null;
};
export type KnowledgeDrafts = {
  noticeHidden: boolean;
  collection: {
    open: boolean;
    name: string;
    scope: string;
    uncertain: boolean;
    conflict: boolean;
  } | null;
  paste: PasteState | null;
  memory: MemoryEditorState | null;
};
export const initialKnowledgeDrafts = (): KnowledgeDrafts => ({
  noticeHidden: false,
  collection: null,
  paste: null,
  memory: null,
});
export function startMemory(document?: MemoryDocument): MemoryEditorState {
  return {
    open: true,
    id: document?.id ?? null,
    expected_revision: document?.revision ?? null,
    draft: document
      ? {
          ...document.draft,
          evidence: document.draft.evidence.map((item) => ({ ...item })),
        }
      : emptyMemory(),
    request: null,
    issue: null,
  };
}
export function startPaste(collection: KnowledgeCollection): PasteState {
  return {
    open: true,
    collection,
    title: "",
    text: "",
    speaker: "",
    observed_at: Math.floor(Date.now() / 1000),
    valid_from: null,
    valid_until: null,
    request: null,
    issue: null,
  };
}
export function memoryProblem(draft: MemoryDraft): string | null {
  if (!draft.subject.trim() || bytes(draft.subject) > 200)
    return "주제를 200바이트 이내로 입력해 주세요.";
  if (!draft.text.trim() || bytes(draft.text) > 4096)
    return "기억 내용은 비어 있지 않은 4KiB 이내의 텍스트여야 합니다.";
  if (draft.evidence.length > 8 || bytes(JSON.stringify(draft)) > 16 * 1024)
    return "근거를 포함한 기억은 16KiB 이내, 근거는 최대 8개까지 저장할 수 있습니다.";
  if (draft.speaker && bytes(draft.speaker) > 200)
    return "화자는 200바이트 이내로 입력해 주세요.";
  if (
    draft.valid_from !== null &&
    draft.valid_until !== null &&
    draft.valid_from >= draft.valid_until
  )
    return "유효 종료 시각은 시작 시각보다 뒤여야 합니다.";
  return null;
}
/** Textarea selection offsets are UTF-16. Evidence offsets are UTF-8 bytes.
 * Never locate an excerpt with indexOf: repeated quotes have distinct spans. */
export function selectedEvidence(
  document: SourceDocument,
  start: number,
  end: number,
): EvidenceRef | null {
  const text = document.text;
  // Textarea values normalize CRLF and bare CR to LF, unlike stored sources.
  const visible = text.replace(/\r\n?/g, "\n");
  if (
    !Number.isInteger(start) ||
    !Number.isInteger(end) ||
    start < 0 ||
    end > visible.length ||
    start >= end
  )
    return null;
  const rawOffset = (offset: number) => {
    let raw = 0;
    let displayed = 0;
    while (displayed < offset) {
      raw += text[raw] === "\r" && text[raw + 1] === "\n" ? 2 : 1;
      displayed++;
    }
    return raw;
  };
  start = rawOffset(start);
  end = rawOffset(end);
  const splitsSurrogate = (offset: number) =>
    offset > 0 &&
    offset < text.length &&
    /[\uD800-\uDBFF]/.test(text[offset - 1]) &&
    /[\uDC00-\uDFFF]/.test(text[offset]);
  if (splitsSurrogate(start) || splitsSurrogate(end)) return null;
  const quote = text.slice(start, end);
  if (!quote.trim() || bytes(quote) > 4096) return null;
  return {
    source_id: document.source.id,
    source_revision: document.source.revision,
    start_byte: bytes(text.slice(0, start)),
    end_byte: bytes(text.slice(0, end)),
    quote,
  };
}
export function localDate(value: number | null) {
  if (value === null) return "";
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) return "";
  const pad = (part: number) => String(part).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}
export const dateInput = (value: string): number | null =>
  value ? Math.floor(new Date(value).getTime() / 1000) : null;
