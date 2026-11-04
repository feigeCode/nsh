import { expect, open, test } from "nsh/test";

test("a test's handlers are the host capabilities the page may call", async () => {
  // The page has no `nsh.json`: the handlers the test answers with are its
  // `capabilities.host`, as an embedding app's own functions are.
  const app = await open("./App.tsx", {
    host: { handlers: { greet: async () => ({ name: "Ada" }) } },
  });
  await app.press("label:greet");
  await app.waitFor("text:greet: Ada");
  // A name the handlers do not answer is not granted.
  await app.press("label:nobody");
  await app.waitFor("text:nobody: ERR_ACCESS_DENIED");
  expect(app.host.calls()).toEqual([{ name: "greet", data: { id: 7 } }]);
});
