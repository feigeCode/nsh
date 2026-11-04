// A page that declares no capabilities of its own: the test that opens it
// plays the embedding app, whose handlers are its host functions.
import { mount, signal, Text, View } from "nsh";
import { host } from "nsh";

function App() {
  const answer = signal("no answer");
  const ask = (name: string) =>
    host.call<{ name: string }>(name, { id: 7 }).then(
      (value) => answer.set(`${name}: ${value.name}`),
      (e: Error & { code?: string }) => answer.set(`${name}: ${e.code}`),
    );
  return (
    <View>
      <Text>{answer()}</Text>
      <View onPress={() => ask("greet")} accessibilityLabel="greet">
        <Text>greet</Text>
      </View>
      <View onPress={() => ask("nobody")} accessibilityLabel="nobody">
        <Text>nobody</Text>
      </View>
    </View>
  );
}

mount(App);
