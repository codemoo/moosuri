# Mobile conversation layout evidence

The change sizes the authenticated workspace to the normal visual viewport,
preserves chat bottom/reading anchors on layout changes, positions the new-response
control above the actual composer, and compacts idle media/voice/library controls.
The long desktop navigation and expanded composer gain their own scrolling.
No API, authentication, saved data, upload or recording dispatch behavior changes.

An independent read-only review found no material regression in the frozen viewport/
anchor implementation, including older-page prepend interaction and zoom behavior.
Root then addressed two actual Chrome findings: the original idle composer consumed
349px at390×844, and desktop account controls extended beyond an800px viewport.

Actual Chrome rendered the real React application with18 synthetic conversations and
loopback API fixtures. After the compact controls, the idle composer measured190px,
leaving159px more conversation area. A multi-line draft and390×480 viewport retained
the bottom within1px; an earlier160px asynchronous layout insertion retained the
older visible run with0px offset. Page-scale1.5 retained the normal layout size.
The desktop sidebar scrolled its account controls back inside the800px frame.
Root visually inspected the resulting390×844 screenshot. These are browser layout
observations, not actual OS virtual keyboard, iOS/Android, VoiceOver/TalkBack or
physical-device certification. No private account/provider was used.

The48 selected existing chat, draft, media and recorder checks passed. Recorder
checks initially queried the old visible caption; their selectors now use the
unchanged accessible button name. Only that failed recorder file was rerun.
TypeScript and production React build passed. The default embedded Linux assistant
binary build passed. No unrelated Rust behavioral suite was repeated.
