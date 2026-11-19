import { useState } from "react";
import type { DraftSetter } from "./KnowledgeForms";
import { initialKnowledgeDrafts, type KnowledgeDrafts } from "./KnowledgeState";
import { KnowledgeView } from "./KnowledgeView";
import "./personal-library.css";
import "./personal-memories.css";

type Props = { accountId: string; onExpired: () => void } & (
  | { drafts: KnowledgeDrafts; updateDrafts: DraftSetter }
  | { drafts?: never; updateDrafts?: never }
);

function MemberMemories({ accountId, onExpired, drafts, updateDrafts }: Props) {
  const [localDrafts, setLocalDrafts] = useState(initialKnowledgeDrafts);
  return (
    <KnowledgeView
      accountId={accountId}
      mode="memory"
      manualMemoriesOnly
      drafts={drafts ?? localDrafts}
      updateDrafts={updateDrafts ?? setLocalDrafts}
      onExpired={onExpired}
    />
  );
}

export function PersonalMemories(props: Props) {
  return <MemberMemories key={props.accountId} {...props} />;
}
