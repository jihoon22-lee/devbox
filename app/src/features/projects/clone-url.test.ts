import { repositoryName } from "./clone-url";

test.each([
  "--upload-pack=evil",
  "ext::evil",
  "evil://host/repo",
  "relative/repo",
  "https://host/repo?token=x",
  "https://token@host/repo.git",
  "ssh://git:secret@host/repo",
  "https://host/..",
  "https://host/repo\n",
])("rejects invalid or credential-bearing URL %s", (url) => {
  expect(repositoryName(url)).toBeUndefined();
});
test.each([
  "https://example.com/team/repo.git",
  "ssh://git@example.com/team/repo.git",
  "git@example.com:team/repo.git",
  "file:///tmp/repo.git",
  "/tmp/repo.git",
])("derives clone folder from %s", (url) => {
  expect(repositoryName(url)).toBe("repo");
});
test("local Git source paths preserve spaces and Unicode", () => {
  expect(repositoryName("/tmp/프로젝트 repo.git")).toBe("프로젝트 repo");
});
