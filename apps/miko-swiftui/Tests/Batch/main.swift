// SPDX-License-Identifier: GPL-3.0-only
import Foundation

struct BatchTestError: LocalizedError { let errorDescription: String? }
let queue = BatchDeviceOperations()
var completions: [String: BatchDeviceOperations.Completion] = [:]
var progress: [String: BatchDeviceOperations.Progress] = [:]
var cancelled: [String] = []
let devices = [("a", "A"), ("b", "B"), ("c", "C")]
queue.begin(.install(path: "/tmp/test.apk"), devices: devices, execute: { _, id, onProgress, done in
    completions[id] = done
    progress[id] = onProgress
}, cancelJob: { cancelled.append($0) })
precondition(queue.running && completions.count == 2)
progress["a"]?("install", "job-a", 0.5)
completions["a"]?(.failure(BatchTestError(errorDescription: "installation failed")))
precondition(completions["c"] != nil, "one failure must not block the rest")
queue.cancel()
precondition(queue.running && queue.cancelling)
progress["b"]?("starting", "job-b", nil)
precondition(cancelled == ["job-b"], "late job id must still be cancelled")
completions["b"]?(.failure(BatchTestError(errorDescription: "cancelled")))
completions["c"]?(.success(()))
precondition(!queue.running)
precondition(queue.results[0].status == .failed("installation failed"))
precondition(queue.results[1].status == .cancelled)
precondition(queue.results[2].status == .succeeded, "a completed request must not be called cancelled")
let staleCompletion = completions["a"]
queue.retry("a")
precondition(queue.running && queue.results[1].status == .cancelled && queue.results[2].status == .succeeded)
staleCompletion?(.failure(BatchTestError(errorDescription: "old result")))
precondition(queue.running, "an old callback must not complete a retry")
completions["a"]?(.success(()))
precondition(!queue.running && queue.results[0].status == .succeeded)

let queued = BatchDeviceOperations()
var neverStarted = [String]()
var finishFirst: BatchDeviceOperations.Completion?
queued.begin(.start(interval: 60), devices: devices, execute: { _, id, _, done in
    neverStarted.append(id); finishFirst = done
}, cancelJob: { _ in })
precondition(neverStarted == ["a"])
queued.cancel()
precondition(queued.results[1].status == .cancelled && queued.results[2].status == .cancelled)
finishFirst?(.success(()))
precondition(!queued.running && neverStarted == ["a"])
print("Batch device coordinator tests passed")

var reserved: Set<String> = ["phone copy", "phone copy 2"]
precondition(BatchCloneNaming.next(source: "Phone", reserved: &reserved) == "Phone Copy 3")
let longName = BatchCloneNaming.next(source: String(repeating: "设备", count: 20), reserved: &reserved)
precondition(longName.utf8.count <= 64 && longName.hasSuffix(" Copy"))
let longGrapheme = "e" + String(repeating: "\u{0301}", count: 30)
reserved.insert("copy")
precondition(BatchCloneNaming.next(source: longGrapheme, reserved: &reserved) == "Copy 2")
let clones = BatchDeviceOperations()
var cloneStarted: [String] = []
var cloneFinish: [String: BatchDeviceOperations.Completion] = [:]
var cloneProgress: [String: BatchDeviceOperations.Progress] = [:]
var cloneCancelled: [String] = []
clones.begin(.clone, devices: devices, execute: { _, id, update, done in
    cloneStarted.append(id); cloneFinish[id] = done; cloneProgress[id] = update
}, cancelJob: { cloneCancelled.append($0) })
precondition(cloneStarted == ["a"])
cloneFinish["a"]?(.success(()))
precondition(cloneStarted == ["a", "b"])
clones.cancel()
cloneProgress["b"]?("copying", "clone-job", nil)
precondition(cloneCancelled == ["clone-job"], "cancel must reach a late clone job")
cloneFinish["b"]?(.failure(BatchTestError(errorDescription: "cancelled")))
precondition(!clones.running && clones.results[1].status == .cancelled && clones.results[2].status == .cancelled)

let uncertain = BatchDeviceOperations()
var uncertainFinish: BatchDeviceOperations.Completion?
uncertain.begin(.install(path: "/tmp/test.apk"), devices: [("a", "A")], execute: { _, _, _, done in uncertainFinish = done }, cancelJob: { _ in })
uncertain.cancel()
uncertainFinish?(.failure(BatchTestError(errorDescription: "cancellation requested but installation result unknown")))
precondition(uncertain.results[0].status == .failed("cancellation requested but installation result unknown"))
