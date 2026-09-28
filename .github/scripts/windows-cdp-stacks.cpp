// Hosted failure-only x64 snapshot stack reader. No dump, locals or raw memory output.
#define _WIN32_WINNT 0x0A00
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <processsnapshot.h>
#include <dbghelp.h>
#include <algorithm>
#include <charconv>
#include <cctype>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>
#pragma comment(lib, "dbghelp.lib")
struct Handle {
  HANDLE value = nullptr;
  ~Handle() { if (value && value != INVALID_HANDLE_VALUE) CloseHandle(value); }
};
struct Snapshot {
  HPSS value = nullptr;
  ~Snapshot() { if (value) PssFreeSnapshot(GetCurrentProcess(), value); }
};
struct Marker {
  HPSSWALK value = nullptr;
  ~Marker() { if (value) PssWalkMarkerFree(value); }
};
struct Symbols {
  HANDLE process;
  bool ready = false;
  ~Symbols() { if (ready) SymCleanup(process); }
};
uint64_t ticks(FILETIME value) {
  return (static_cast<uint64_t>(value.dwHighDateTime) << 32) | value.dwLowDateTime;
}
uint64_t created(HANDLE process) {
  FILETIME birth{}, exit{}, kernel{}, user{};
  return GetProcessTimes(process, &birth, &exit, &kernel, &user) ? ticks(birth) : 0;
}
std::string module_name(const char* path) {
  std::string name(path);
  name = name.substr(name.find_last_of("/\\") + 1);
  std::transform(name.begin(), name.end(), name.begin(), [](unsigned char c) { return static_cast<char>(tolower(c)); });
  const char* known[] = {"ntdll.dll", "kernel32.dll", "kernelbase.dll", "user32.dll", "win32u.dll", "combase.dll", "ole32.dll", "oleaut32.dll", "rpcrt4.dll", "uiautomationcore.dll", "dwmapi.dll", "msedge.dll", "msedgewebview2.exe", "embeddedbrowserwebview.dll", "webview2loader.dll", "devbox-api-studio.exe", "windows-cdp-stacks.exe"};
  for (const auto* candidate : known) if (name == candidate) return name.substr(0, name.find_last_of('.'));
  return "other";
}
bool safe_symbol(const std::string& name) {
  if (name.empty() || name.size() > 160) return false;
  for (unsigned char c : name) if (!isalnum(c) && std::strchr("_?$@:<>, ()&*~!+-.", c) == nullptr) return false;
  return true;
}
struct Frame { std::string module; std::string symbol; uint64_t offset; };
struct Result { const char* state; std::vector<Frame> frames; };
Result capture(DWORD pid, uint64_t expected, DWORD requested) {
  Handle process{OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_CREATE_PROCESS | PROCESS_DUP_HANDLE | SYNCHRONIZE, FALSE, pid)};
  if (!process.value) return {"denied", {}};
  if (!expected || created(process.value) != expected) return {"identity_changed", {}};
  Snapshot snapshot;
  const auto flags = static_cast<PSS_CAPTURE_FLAGS>(PSS_CAPTURE_VA_CLONE | PSS_CAPTURE_THREADS | PSS_CAPTURE_THREAD_CONTEXT);
  if (PssCaptureSnapshot(process.value, flags, CONTEXT_FULL, &snapshot.value) != ERROR_SUCCESS) return {"capture_failed", {}};
  PSS_VA_CLONE_INFORMATION clone{};
  if (PssQuerySnapshot(snapshot.value, PSS_QUERY_VA_CLONE_INFORMATION, &clone, sizeof(clone)) != ERROR_SUCCESS) return {"capture_failed", {}};
  Marker marker;
  if (PssWalkMarkerCreate(nullptr, &marker.value) != ERROR_SUCCESS) return {"capture_failed", {}};
  CONTEXT context{};
  DWORD selected = 0;
  uint64_t oldest = UINT64_MAX;
  for (unsigned count = 0; count < 256; ++count) {
    PSS_THREAD_ENTRY entry{};
    if (PssWalkSnapshot(snapshot.value, PSS_WALK_THREADS, marker.value, &entry, sizeof(entry)) != ERROR_SUCCESS) break;
    if (entry.ProcessId != pid || entry.ExitStatus != STILL_ACTIVE || !entry.ContextRecord || entry.SizeOfContextRecord < sizeof(CONTEXT)) continue;
    if ((requested && entry.ThreadId == requested) || (!requested && ticks(entry.CreateTime) < oldest)) {
      context = *entry.ContextRecord;
      selected = entry.ThreadId;
      oldest = ticks(entry.CreateTime);
      if (requested) break;
    }
  }
  if (!selected) return {"thread_unavailable", {}};
  // No manual thread suspension: walk the immutable clone after OS capture.
  SymSetOptions(SYMOPT_DEFERRED_LOADS | SYMOPT_UNDNAME | SYMOPT_FAIL_CRITICAL_ERRORS | SYMOPT_NO_PROMPTS | SYMOPT_IGNORE_NT_SYMPATH);
  Symbols symbols{clone.VaCloneHandle};
  symbols.ready = SymInitialize(clone.VaCloneHandle, "", TRUE) != FALSE;
  if (!symbols.ready) return {"symbols_unavailable", {}};
  STACKFRAME64 frame{};
  frame.AddrPC.Offset = context.Rip;
  frame.AddrStack.Offset = context.Rsp;
  frame.AddrFrame.Offset = context.Rbp;
  frame.AddrPC.Mode = frame.AddrStack.Mode = frame.AddrFrame.Mode = AddrModeFlat;
  std::vector<Frame> frames;
  DWORD64 last = 0;
  for (unsigned count = 0; count < 32 && frame.AddrPC.Offset; ++count) {
    if (!StackWalk64(IMAGE_FILE_MACHINE_AMD64, clone.VaCloneHandle, reinterpret_cast<HANDLE>(static_cast<uintptr_t>(selected)), &frame, &context, nullptr, SymFunctionTableAccess64, SymGetModuleBase64, nullptr)) break;
    const auto address = frame.AddrPC.Offset;
    if (address == last) break;
    last = address;
    IMAGEHLP_MODULE64 module{};
    module.SizeOfStruct = sizeof(module);
    if (SymGetModuleInfo64(clone.VaCloneHandle, address, &module) && address >= module.BaseOfImage) {
      Frame output{module_name(module.ImageName), "", address - module.BaseOfImage};
      alignas(SYMBOL_INFO) char buffer[sizeof(SYMBOL_INFO) + 512]{};
      auto* info = reinterpret_cast<SYMBOL_INFO*>(buffer);
      info->SizeOfStruct = sizeof(SYMBOL_INFO);
      info->MaxNameLen = 512;
      DWORD64 displacement = 0;
      if (output.module != "other" && SymFromAddr(clone.VaCloneHandle, address, &displacement, info) && info->NameLen <= 160) {
        std::string name(info->Name, info->NameLen);
        if (safe_symbol(name)) output.symbol = name;
      }
      frames.push_back(output);
    }
  }
  if (created(process.value) != expected || WaitForSingleObject(process.value, 0) == WAIT_OBJECT_0) return {"identity_changed", {}};
  return {frames.empty() ? "walk_failed" : "observed", frames};
}
void print(const Result& result) {
  std::cout << "{\"state\":\"" << result.state << "\",\"frames\":[";
  bool comma = false;
  for (const auto& frame : result.frames) {
    if (comma) std::cout << ',';
    comma = true;
    std::cout << "{\"module\":\"" << frame.module << "\",\"symbol\":";
    if (frame.symbol.empty()) std::cout << "null"; else std::cout << '"' << frame.symbol << '"';
    std::cout << ",\"offset\":" << frame.offset << '}';
  }
  std::cout << "]}\n";
}
struct ProbeEvents { HANDLE ready; HANDLE done; };
__declspec(noinline) DWORD WINAPI ProbeWait(void* value) {
  const auto* events = static_cast<ProbeEvents*>(value);
  SetEvent(events->ready);
  WaitForSingleObject(events->done, INFINITE);
  return 0;
}
DWORD WINAPI Watchdog(void* done) {
  if (WaitForSingleObject(done, 10000) == WAIT_TIMEOUT) TerminateProcess(GetCurrentProcess(), 3);
  return 0;
}
struct Deadline {
  Handle done{CreateEventW(nullptr, TRUE, FALSE, nullptr)};
  Handle thread{done.value ? CreateThread(nullptr, 0, Watchdog, done.value, 0, nullptr) : nullptr};
  ~Deadline() { if (done.value) SetEvent(done.value); if (thread.value) WaitForSingleObject(thread.value, INFINITE); }
};
int main(int argc, char** argv) {
  Deadline deadline;
  if (!deadline.thread.value) return 3;
  if (argc == 2 && std::string(argv[1]) == "--self-test") {
    Handle event{CreateEventW(nullptr, TRUE, FALSE, nullptr)};
    Handle ready{CreateEventW(nullptr, TRUE, FALSE, nullptr)};
    if (!event.value || !ready.value) return 1;
    ProbeEvents events{ready.value, event.value};
    DWORD tid = 0;
    Handle thread{CreateThread(nullptr, 0, ProbeWait, &events, 0, &tid)};
    if (!thread.value || WaitForSingleObject(ready.value, 5000) != WAIT_OBJECT_0) return 1;
    const auto result = capture(GetCurrentProcessId(), created(GetCurrentProcess()), tid);
    SetEvent(event.value);
    WaitForSingleObject(thread.value, 5000);
    print(result);
    const auto rejected = capture(GetCurrentProcessId(), created(GetCurrentProcess()) + 1, tid);
    return result.frames.size() >= 2 && std::string(rejected.state) == "identity_changed" ? 0 : 1;
  }
  if (argc != 4) return 2;
  uint64_t args[3]{};
  for (int i = 0; i < 3; ++i) {
    const auto* end = argv[i + 1] + std::strlen(argv[i + 1]);
    const auto parsed = std::from_chars(argv[i + 1], end, args[i]);
    if (parsed.ec != std::errc{} || parsed.ptr != end) return 2;
  }
  if (!args[0] || args[0] > MAXDWORD || !args[1] || args[2] > MAXDWORD) return 2;
  print(capture(static_cast<DWORD>(args[0]), args[1], static_cast<DWORD>(args[2])));
  return 0;
}
