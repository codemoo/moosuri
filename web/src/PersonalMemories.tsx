import { useState } from "react";
import { initialKnowledgeDrafts } from "./KnowledgeState";
import { KnowledgeView } from "./KnowledgeView";
import "./personal-library.css";

type Props = { accountId: string; onExpired: () => void };

function MemberMemories({ accountId, onExpired }: Props) {
  const [drafts, setDrafts] = useState(initialKnowledgeDrafts);
  return (
    <KnowledgeView
      accountId={accountId}
      mode="memory"
      manualMemoriesOnly
      drafts={drafts}
      updateDrafts={setDrafts}
      onExpired={onExpired}
    />
  );
}

export function PersonalMemories(props: Props) {
  return <MemberMemories key={props.accountId} {...props} />;
}
