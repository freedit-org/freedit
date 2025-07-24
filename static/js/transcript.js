const audio = document.getElementById("audio");
const transcriptDiv = document.getElementById("transcript");
let transcriptData = [];
const transcriptWrapper = document.getElementById("transcript-wrapper");

const audioUrl = audio?.dataset.audioUrl;
const srtUrl = audio?.dataset.srtUrl;

if (typeof audioUrl !== "undefined" && audioUrl) {
  audio.src = audioUrl;
  audio.load();
}

if (typeof srtUrl !== "undefined" && srtUrl) {
  fetch(srtUrl)
    .then((response) => response.text())
    .then((srtText) => {
      parseSRT(srtText);
      renderTranscript();
    })
    .catch((err) => {
      transcriptDiv.innerHTML = "Failed to load transcript.";
      console.error("SRT load error:", err);
    });
} else {
  transcriptDiv.innerHTML = "No SRT URL provided.";
}

function parseTime(s) {
  const [h, m, rest] = s.split(":");
  const [sec, ms] = rest.split(",");
  return (
    parseInt(h) * 3600 + parseInt(m) * 60 + parseInt(sec) + parseInt(ms) / 1000
  );
}

function parseSRT(srt) {
  transcriptData = [];
  const blocks = srt.trim().split(/\n\s*\n/);
  for (const block of blocks) {
    const lines = block.trim().split("\n");
    if (lines.length < 2) continue;
    const timeMatch = lines[1].match(
      /(\d{2}:\d{2}:\d{2},\d{3}) --> (\d{2}:\d{2}:\d{2},\d{3})/,
    );
    if (!timeMatch) continue;
    const start = parseTime(timeMatch[1]);
    const end = parseTime(timeMatch[2]);
    const text = lines.slice(2).join(" ");
    const wordMatch = text.match(/<u>(.*?)<\/u>/);
    if (!wordMatch) continue;
    transcriptData.push({
      start,
      end,
      word: wordMatch[1],
      fullText: text.replace(/<\/?.*?>/g, ""),
    });
  }
}

function renderTranscript() {
  transcriptDiv.innerHTML = "";
  transcriptData.forEach((entry, i) => {
    const span = document.createElement("span");
    span.textContent = entry.word + " ";
    span.className = "word";
    span.id = "word-" + i;
    span.dataset.start = entry.start;
    span.dataset.end = entry.end;
    transcriptDiv.appendChild(span);
  });
}

audio.ontimeupdate = () => {
  const time = audio.currentTime;
  transcriptData.forEach((entry, i) => {
    const el = document.getElementById("word-" + i);
    if (!el) return;
    if (time >= entry.start && time <= entry.end) {
      el.classList.add("has-background-warning");
      el.scrollIntoView({
        behavior: "smooth",
        block: "center",
        inline: "nearest",
      });
    } else {
      el.classList.remove("has-background-warning");
    }
  });
};
