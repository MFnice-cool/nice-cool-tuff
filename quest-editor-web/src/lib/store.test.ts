import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { parseSnbt, serializeSnbt } from "./snbt";
import {
  applyLocaleOverlay,
  exportChapterSnbt,
  formatLoadMessage,
  loadQuestBookFromSnbt,
  summarizeQuestBookLoad,
  type QuestBook,
} from "./store";

describe("locale overlay + folder load", () => {
  it("applies lang titles when chapter/quest SNBT has no title", () => {
    const files = new Map<string, string>([
      [
        "data.snbt",
        `{
	default_quest_shape: "circle"
	version: 13
}`,
      ],
      [
        "chapter_groups.snbt",
        `{
	chapter_groups: [
		{ id: "G1" }
	]
}`,
      ],
      [
        "chapters/welcome.snbt",
        `{
	filename: "welcome"
	id: "CH1"
	order_index: 0
	quests: [
		{
			id: "Q1"
			tasks: [{ id: "T1" type: "checkmark" }]
			x: 0.0d
			y: 0.0d
		}
	]
}`,
      ],
      [
        "lang/en_us.snbt",
        `{
	file.0000000000000001.title: "Skybound Test Pack"
	chapter.CH1.title: " &f&lWelcome&r&r"
	chapter_group.G1.title: "Aeronautics"
	quest.Q1.title: "Meet the crew"
	quest.Q1.quest_subtitle: "Intro"
	quest.Q1.quest_desc: ["Hello world"]
	task.T1.title: "Check me"
}`,
      ],
      [
        "reward_tables/tier1.snbt",
        `{
	id: "RT1"
	loot_size: 1
	order_index: 0
	rewards: []
}`,
      ],
    ]);

    const book = loadQuestBookFromSnbt(files);
    expect(book.chapters).toHaveLength(1);
    expect(book.title).toBe("Skybound Test Pack");
    expect(book.chapterGroups[0]?.title).toBe("Aeronautics");
    expect(book.chapters[0]?.title).toContain("Welcome");
    expect(book.chapters[0]?.titleFromSnbt).toBe(false);
    expect(book.chapters[0]?.quests[0]?.title).toBe("Meet the crew");
    expect(book.chapters[0]?.quests[0]?.subtitle).toBe("Intro");
    expect(book.chapters[0]?.quests[0]?.description).toEqual(["Hello world"]);
    expect(book.chapters[0]?.quests[0]?.tasks[0]?.title).toBe("Check me");
    expect(book.rewardTables).toHaveLength(1);
    expect(book.activeLocale).toBe("en_us");

    const snbt = exportChapterSnbt(book.chapters[0]!);
    expect(snbt).not.toMatch(/\btitle:\s*"Meet the crew"/);
    expect(snbt).not.toMatch(/chapter.*Welcome/);
    // chapter title omitted (locale-only)
    expect(snbt).toMatch(/\bid:\s*"CH1"/);

    const msg = formatLoadMessage(summarizeQuestBookLoad(book, files.size));
    expect(msg).toContain("1 chapter");
    expect(msg).toContain("lang key");
    expect(msg).toContain("reward table");
  });

  it("keeps inline titles over lang when titleFromSnbt", () => {
    const book: QuestBook = {
      chapters: [
        {
          id: "CH1",
          title: "Inline Chapter",
          titleFromSnbt: true,
          quests: [
            {
              id: "Q1",
              title: "Inline Quest",
              titleFromSnbt: true,
              dependencies: [],
              tasks: [],
              rewards: [],
              optional: false,
              x: 0,
              y: 0,
            },
          ],
        },
      ],
      chapterGroups: [],
      locales: {
        en_us: {
          "chapter.CH1.title": "Lang Chapter",
          "quest.Q1.title": "Lang Quest",
        },
      },
    };
    applyLocaleOverlay(book);
    expect(book.chapters[0]?.title).toBe("Inline Chapter");
    expect(book.chapters[0]?.quests[0]?.title).toBe("Inline Quest");
  });

  it("parses typed int arrays [I; ...]", () => {
    const v = parseSnbt(`{
	id: [I;
		1
		-2
		3
	]
}`) as Record<string, unknown>;
    expect(v.id).toEqual({ __snbtArray: "I", values: [1, -2, 3] });
    const out = serializeSnbt(v);
    expect(out).toContain("[I;");
    expect(serializeSnbt({ flag: true })).toContain("true");
    expect(serializeSnbt({ flag: true })).not.toContain("1b");
  });
});

const SKYBOUND_QUESTS =
  "D:/PrismLauncher-Windows-MinGW-w64-Portable-11.0.3/instances/Create Aeronautics - Skybound - Create Aeronautics & Create Addons with Quests/minecraft/config/ftbquests/quests";

describe.runIf(existsSync(SKYBOUND_QUESTS))("Skybound pack smoke", () => {
  function collectSnbt(dir: string, prefix = ""): Map<string, string> {
    const map = new Map<string, string>();
    for (const name of readdirSync(dir, { withFileTypes: true })) {
      const rel = prefix ? `${prefix}/${name.name}` : name.name;
      const full = join(dir, name.name);
      if (name.isDirectory()) {
        for (const [k, v] of collectSnbt(full, rel)) map.set(k, v);
      } else if (name.name.endsWith(".snbt")) {
        map.set(rel.replace(/\\/g, "/"), readFileSync(full, "utf8"));
      }
    }
    return map;
  }

  it("loads full quests folder with lang overlay", () => {
    const files = collectSnbt(SKYBOUND_QUESTS);
    const book = loadQuestBookFromSnbt(files);
    expect(book.chapters.length).toBeGreaterThanOrEqual(10);
    expect(book.locales?.en_us).toBeTruthy();
    expect(Object.keys(book.locales!.en_us!).length).toBeGreaterThan(1000);
    expect(book.title?.toLowerCase()).toContain("aeronautics");
    expect(book.chapterGroups.length).toBeGreaterThanOrEqual(2);
    expect(book.rewardTables!.length).toBeGreaterThan(0);
    const welcome = book.chapters.find((c) => c.filename === "welcome" || c.id === "714BED084D92B8FB");
    expect(welcome).toBeTruthy();
    expect(welcome!.title.toLowerCase()).toContain("welcome");
    expect(welcome!.quests.length).toBeGreaterThan(0);
    expect(welcome!.quests.some((q) => q.title && q.title !== "Quest")).toBe(true);
  });
});
