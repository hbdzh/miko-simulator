// SPDX-License-Identifier: GPL-3.0-only
import Foundation
import Combine

enum BatchDeviceOperation {
    case start(interval: TimeInterval)
    case stop
    case install(path: String)
    case clone
}

enum BatchCloneNaming {
    static func next(source: String, reserved: inout Set<String>) -> String {
        var index = 1
        while true {
            let suffix = index == 1 ? " Copy" : " Copy \(index)"
            var base = source
            while (base + suffix).utf8.count > 64 && !base.isEmpty { base.removeLast() }
            let name = base.isEmpty ? String(suffix.dropFirst()) : base + suffix
            if reserved.insert(name.lowercased()).inserted { return name }
            index += 1
        }
    }
}

enum BatchDeviceStatus: Equatable {
    case queued
    case running(message: String, fraction: Double?)
    case succeeded
    case failed(String)
    case cancelled
}

struct BatchDeviceResult: Identifiable {
    let id: String
    let name: String
    var status: BatchDeviceStatus
}

/// Main-thread coordinator. A cancellation prevents queued work from starting
/// and requests cancellation of daemon jobs; already-running non-job requests
/// still report their actual outcome.
final class BatchDeviceOperations: ObservableObject {
    typealias Progress = (String, String?, Double?) -> Void
    typealias Completion = (Result<Void, Error>) -> Void
    typealias Execute = (BatchDeviceOperation, String, @escaping Progress, @escaping Completion) -> Void

    @Published private(set) var results: [BatchDeviceResult] = []
    @Published private(set) var running = false
    @Published private(set) var cancelling = false
    private(set) var operation: BatchDeviceOperation?
    private var pending: [String] = []
    private var active = Set<String>()
    private var jobs: [String: String] = [:]
    private var cancelledJobs = Set<String>()
    private var scheduled: DispatchWorkItem?
    private var nextStart: TimeInterval = 0
    private var execute: Execute?
    private var cancelJob: ((String) -> Void)?
    private var pumping = false
    private var generation = UUID()
    private let maxConcurrent = 2

    func begin(_ operation: BatchDeviceOperation, devices: [(String, String)], execute: @escaping Execute, cancelJob: @escaping (String) -> Void) {
        guard !running, !devices.isEmpty else { return }
        var seen = Set<String>()
        let unique = devices.filter { seen.insert($0.0).inserted }
        generation = UUID(); self.operation = operation; self.execute = execute; self.cancelJob = cancelJob
        results = unique.map { BatchDeviceResult(id: $0.0, name: $0.1, status: .queued) }
        pending = unique.map(\.0); active.removeAll(); jobs.removeAll(); cancelledJobs.removeAll()
        nextStart = 0; cancelling = false; running = true
        pump()
    }

    func cancel() {
        guard running, !cancelling else { return }
        cancelling = true
        scheduled?.cancel(); scheduled = nil
        for id in pending { update(id, .cancelled) }
        pending.removeAll()
        for job in jobs.values { requestCancellation(job) }
        finishIfDone()
    }

    func retry(_ id: String) {
        guard !running, operation != nil, execute != nil, cancelJob != nil,
              let result = results.first(where: { $0.id == id }) else { return }
        switch result.status {
        case .failed, .cancelled: break
        default: return
        }
        generation = UUID(); pending = [id]; active.removeAll(); jobs.removeAll(); cancelledJobs.removeAll()
        nextStart = 0; cancelling = false; running = true; update(id, .queued)
        pump()
    }

    private func requestCancellation(_ job: String) {
        guard cancelledJobs.insert(job).inserted else { return }
        cancelJob?(job)
    }

    private func update(_ id: String, _ status: BatchDeviceStatus) {
        guard let index = results.firstIndex(where: { $0.id == id }) else { return }
        results[index].status = status
    }

    private func pump() {
        guard running, !cancelling, !pumping, let operation, let execute else { return }
        pumping = true
        defer { pumping = false; finishIfDone() }
        let limit: Int
        if case .clone = operation { limit = 1 } else { limit = maxConcurrent }
        while active.count < limit, !pending.isEmpty, !cancelling {
            let now = ProcessInfo.processInfo.systemUptime
            if nextStart > now {
                if scheduled == nil {
                    let token = generation
                    let work = DispatchWorkItem { [weak self] in
                        guard let self, self.generation == token else { return }
                        self.scheduled = nil; self.pump()
                    }
                    scheduled = work
                    DispatchQueue.main.asyncAfter(deadline: .now() + (nextStart - now), execute: work)
                }
                return
            }
            let id = pending.removeFirst()
            active.insert(id); update(id, .running(message: "", fraction: nil))
            if case .start(let interval) = operation { nextStart = now + max(0, interval) }
            let token = generation
            execute(operation, id, { [weak self] message, job, fraction in
                guard let self, self.generation == token, self.active.contains(id) else { return }
                if let job {
                    self.jobs[id] = job
                    if self.cancelling { self.requestCancellation(job) }
                }
                self.update(id, .running(message: message, fraction: fraction))
            }, { [weak self] result in
                guard let self, self.generation == token, self.active.remove(id) != nil else { return }
                self.jobs.removeValue(forKey: id)
                switch result {
                case .success: self.update(id, .succeeded)
                case .failure(let error):
                    let message = error.localizedDescription
                    let cancellation = ["cancelled", "canceled", "操作已取消", "已取消"].contains(message.trimmingCharacters(in: .whitespacesAndNewlines).lowercased())
                    self.update(id, self.cancelling && cancellation ? .cancelled : .failed(message))
                }
                self.finishIfDone()
                self.pump()
            })
        }
    }

    private func finishIfDone() {
        if pending.isEmpty && active.isEmpty {
            scheduled?.cancel(); scheduled = nil; running = false; cancelling = false
        }
    }
}
