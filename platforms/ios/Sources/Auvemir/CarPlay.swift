import CarPlay
import Foundation
import SQLite3

@objc(AuvemirCarPlaySceneDelegate)
final class CarPlaySceneDelegate: NSObject, CPTemplateApplicationSceneDelegate {
    private var controller: CPInterfaceController?
    private let reads = DispatchQueue(label: "org.auvemir.carplay.library", qos: .userInitiated)
    private struct Entry: Sendable { let id: String; let title: String; let artist: String }

    func templateApplicationScene(_ scene: CPTemplateApplicationScene, didConnect interfaceController: CPInterfaceController) {
        controller = interfaceController
        let categories = [("local_music", "Music"), ("online_music", "Online music"),
            ("audiobook", "Audiobooks"), ("podcast", "Podcasts"), ("radio", "Radio"), ("recording", "Recordings")]
        let items = categories.map { kind, title in
            let item = CPListItem(text: title, detailText: nil)
            item.handler = { [weak self] _, done in self?.open(kind: kind, title: title, offset: 0); done() }
            return item
        }
        let root = CPListTemplate(title: "Auvemir", sections: [CPListSection(items: items)])
        let now = CPBarButton(type: .text) { [weak self] _ in
            self?.controller?.pushTemplate(CPNowPlayingTemplate.shared, animated: true, completion: nil)
        }
        now.title = "Now Playing"
        root.trailingNavigationBarButtons = [now]
        interfaceController.setRootTemplate(root, animated: false, completion: nil)
    }

    func templateApplicationScene(_ scene: CPTemplateApplicationScene, didDisconnect interfaceController: CPInterfaceController) {
        controller = nil
    }

    private func open(kind: String, title: String, offset: Int) {
        let list = CPListTemplate(title: title, sections: [CPListSection(items: [CPListItem(text: "Loading…", detailText: nil)])])
        controller?.pushTemplate(list, animated: true, completion: nil)
        let path = dataDirectory().appendingPathComponent("auvemir.sqlite").path
        reads.async { [weak self] in
            let entries = Self.read(path: path, kind: kind, offset: offset)
            DispatchQueue.main.async {
                guard let self, self.controller != nil else { return }
                var items = entries.map { entry in
                    let item = CPListItem(text: entry.title, detailText: entry.artist)
                    item.handler = { [weak self] _, done in
                        command("play-id", entry.id)
                        self?.controller?.pushTemplate(CPNowPlayingTemplate.shared, animated: true, completion: nil)
                        done()
                    }
                    return item
                }
                if entries.count == 100 {
                    let more = CPListItem(text: "More…", detailText: nil)
                    more.handler = { [weak self] _, done in self?.open(kind: kind, title: title, offset: offset + 100); done() }
                    items.append(more)
                }
                if items.isEmpty { items = [CPListItem(text: "No audio here yet", detailText: "Add audio in Auvemir on your phone.")] }
                list.updateSections([CPListSection(items: items)])
            }
        }
    }

    nonisolated private static func read(path: String, kind: String, offset: Int) -> [Entry] {
        var db: OpaquePointer?
        guard sqlite3_open_v2(path, &db, SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX, nil) == SQLITE_OK else {
            if db != nil { sqlite3_close(db) }; return []
        }
        defer { sqlite3_close(db) }
        sqlite3_busy_timeout(db, 250)
        var query: OpaquePointer?
        guard sqlite3_prepare_v2(db, "SELECT id,title,artist FROM media WHERE kind=?1 ORDER BY album COLLATE NOCASE,track_number,title COLLATE NOCASE LIMIT 100 OFFSET ?2", -1, &query, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(query) }
        _ = kind.withCString { sqlite3_bind_text(query, 1, $0, -1, unsafeBitCast(-1, to: sqlite3_destructor_type.self)) }
        sqlite3_bind_int64(query, 2, Int64(offset))
        var entries: [Entry] = []
        while sqlite3_step(query) == SQLITE_ROW {
            func text(_ column: Int32) -> String {
                guard let value = sqlite3_column_text(query, column) else { return "" }
                return String(cString: value)
            }
            entries.append(Entry(id: text(0), title: text(1), artist: text(2)))
        }
        return entries
    }
}
