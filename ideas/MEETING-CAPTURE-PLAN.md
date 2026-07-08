# MEETING-CAPTURE-PLAN — local, self-enabled call transcription → everything

A sibling to DICTATION-PLAN.md, but a **distinct feature**. Dictation is
*mic → text into the focused window* (push-to-talk, short). This is *whole
call → transcript file*, self-triggered, landing directly in the
`../everything` knowledge repo — bypassing the Microsoft/Teams transcript
dependency that makes getting transcripts today a pain.

Status: **design done, not started.** Decisions open at the bottom.

Consent is already covered by employment terms; the org already runs AI
meeting transcription. This is purely about *self-service + directness*:
enable it myself, get the text immediately, into my own pipeline.

## The key discovery: everything already ingests transcripts

The seam is already built — we plug into it, we don't invent it.

`~/dev/everything` (`evctl` / the `fetch` lib) already treats **transcripts**
as a first-class sync stream (`make sync WHAT=transcripts FROM=…`). Today it
pulls them from Microsoft Graph:

- `GET /me/onlineMeetings/{id}/transcripts` → list, then
  `…/transcripts/{tid}/content?$format=text/vtt` → **WebVTT** content.
- Written to **`.inbox/transcripts/YYYY-MM-DD-<slugged-subject>.vtt`**
  (gitignored, transient like mail bodies).
- The calendar event carries a **`transcript_file`** field (relative path of
  that VTT, or `null`).
- Curation distills the VTT into the meeting's `notes` in
  `ontology/meetings/*.ron`, then the raw VTT is discarded.

The limits — i.e. exactly why local capture is worth it:

- **Teams-only.** `GET /me/onlineMeetings/...` is Graph/Teams. A Google Meet
  or Zoom call gets no transcript — the sample `2026-06-01-...-credeq.ron`
  shows `location: Some("Google Meet")`, `source: None`, `teams: false`: a
  real meeting with no transcript.
- **Only if transcription was running**, admin-consented scopes
  (`OnlineMeetingTranscript.Read.All`), and delayed until you next sync.

**So the whole feature reduces to:** produce a VTT locally, name it the way
everything expects, drop it in `.inbox/transcripts/`. The existing curation
pipeline ingests it with **zero changes**. whisper.cpp emits VTT natively
(`--output-vtt`), so the formats already match.

## The design

Two halves, split cleanly across the two repos by ownership.

### battlestation side — capture (the new work)

The desktop owns audio + hotkey + local STT. This is where DICTATION-PLAN's
recon pays off (PipeWire ✓, ffmpeg ✓, Intel Arc B390 = Vulkan/CPU whisper).

1. **Capture both sides of the call via PipeWire.** The other participants
   are captured from the **`.monitor`** source of the sink the call app plays
   to; your voice from the mic. A PipeWire **loopback / combined virtual
   source** mixes `call-output + mic` into one stream — the full conversation
   in a single feed for the transcriber. (This is the piece dictation didn't
   need — dictation is mic-only.)
2. **Transcribe with whisper.cpp** (same engine family as voxtype), output
   `--output-vtt`. CPU is likely fine for after-the-fact processing; Intel
   Vulkan if we want near-realtime. Larger model than dictation wants —
   accuracy over latency, since it's not typing live.
3. **A toggle to start/stop a meeting recording** — a Hypr keybind or a
   `bsctl`-style verb ("I'm in a meeting, capture it"). Records audio to a
   temp file (or streams), transcribes on stop.
4. **Name + drop the VTT into `../everything/.inbox/transcripts/`** matching
   everything's convention so it's auto-attributed to the right calendar
   event (see attribution note below).

### everything side — ingestion (mostly already done)

- The VTT format, the `.inbox/transcripts/` location, and the curation into
  `meetings/*.ron` **already exist**. Ideally: no code change to consume a
  well-named local VTT.
- Small additions worth considering (everything's call, not battlestation's):
  a `source: Some("local")` vs Teams marker on the meeting so provenance is
  legible; and an attribution path for **dropped-in** files that doesn't
  assume the Graph occurrence-matching that ran when *it* fetched them.

## The attribution subtlety (the one real integration question)

everything attributes a transcript to a calendar event by name +
time-window. `transcript_file_name(start, subject, event_id)` bakes in a
suffix derived from the **event id** (tests show `2026-07-06-exco-weekly-…`
and an `-untitled-` fallback). A locally-produced VTT won't know that event
id unless we tell it. Two ways to close the gap, cleanly:

- **Preferred — capture reads the agenda.** everything already fetches a
  14-day look-ahead into `.inbox/agenda/events.json` (`make agenda`). The
  capture tool queries "what meeting is happening *now*?" from that, and
  names the VTT with the matching event's id/subject — so it lands
  pre-attributed, exactly as a Graph fetch would have.
- **Fallback — time-window match on ingest.** Drop the VTT named by
  wall-clock start; teach everything to attach any unclaimed local VTT to the
  calendar occurrence whose window contains it (the same
  `[start, end + 6h]` logic `pick_transcript_for` already implements — just
  sourced from a file instead of Graph).

## Speaker identification (diarization)

Who-said-what has three tiers here, cheapest first:

1. **Channel separation — free, and the biggest win.** Because capture is via
   PipeWire, keep the **mic** (you) and the **call output** (everyone else) as
   *separate streams*, transcribe each, merge by timestamp. Perfect "me vs.
   them" labelling with zero ML. This is the natural output of the loopback
   design and should be the v1 behaviour.
2. **whisper.cpp `-tdrz` (tinydiarize) — turn marks only.** whisper.cpp's
   built-in diarization is *turn segmentation*: it emits `[SPEAKER_TURN]` when
   the speaker changes but does **not** cluster/identify the same person
   across the call, and is finetuned for the **`small.en` English model only**.
   Useful to mark turn boundaries *within* the remote side ("Speaker A/B/C"
   unnamed), stacked on top of the channel split.
3. **True diarization (named/clustered speakers) — heavy, awkward here.**
   Real labelled diarization means **pyannote.audio**, usually via
   **whisperX**. It's PyTorch/CUDA-oriented; on GPU ~1 min, but forced to
   **CPU it's ~2h for 24 min of audio**. The **Intel Arc B390 has no CUDA**
   (and pyannote wants 16GB+ VRAM), so there's no easy GPU path — CPU-only is
   likely too slow even for after-the-call use. Park this unless it proves
   necessary.

Asymmetry worth keeping in mind: the **Teams/Graph transcripts everything
already fetches carry real participant *names*** (Teams knows the roster).
Local capture trades named speakers for coverage + immediacy — Graph stays
better for named speakers on a Teams call; local wins for Google Meet / Zoom
or instant turnaround. The channel split (tier 1) already recovers the single
most useful distinction (you vs. the room) without any of that.

## Why this is a strong idea, beyond convenience

- **Covers what Graph can't** — Google Meet, Zoom, any audio through the
  speakers. The `source: None` meetings stop being blind spots.
- **Immediate + self-owned** — no waiting on org transcription or a sync; the
  transcript exists the moment the call ends, in the repo that already turns
  it into people/meetings/actions knowledge.
- **Reuses the whole back half** — everything's curation, ontology, and
  provenance are already built; this only feeds them a new source.
- **Shares the dictation stack** — whisper.cpp + PipeWire are the same
  primitives DICTATION-PLAN installs; marginal cost is the audio routing and
  the drop-in glue.

## Open decisions

1. **Where does the capture tool live?** A battlestation script/verb, or a
   small tool in everything's `tools/` next to `evctl`? (Capture is a desktop
   concern → leans battlestation; the naming/attribution logic is everything's
   domain → leans there. Could split: battlestation captures audio+VTT,
   everything owns the drop-in/attribution.)
2. **Realtime vs. after-the-call.** Stream-transcribe live (Vulkan, more
   complexity) or record audio and transcribe once on stop (simpler, CPU-fine,
   a short wait). After-the-call is the obvious v1.
3. **Attribution: agenda-read vs. ingest-time window match** (above).
4. **Provenance marker** — add `source: local` to distinguish self-captured
   from Teams transcripts in the ontology?
5. **Audio routing UX** — a fixed combined source always available, or set up
   on demand when capture starts? Auto-detect which sink the call app uses?
6. **Retention** — keep the raw audio, or discard after transcription like the
   VTT is discarded post-curation?

Effort: **medium.** Bigger than dictation (PipeWire loopback + the
cross-repo naming/attribution glue is real work), but the transcription
engine and the entire ingestion/curation back-end already exist. No new
knowledge-base machinery — capture + route + name + drop.
