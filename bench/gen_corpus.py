#!/usr/bin/env python3
"""Benchmark corpus generator for textlint vs tsuzulint throughput tests.

Generates a corpus of representative Japanese technical-writing documents under
``bench/corpus/``.  All prose is original (hand-written here) to keep the corpus
copyright-safe; nothing is copied from any external source.

Design goals
------------
* Deterministic: a fixed ``random.Random(SEED)`` drives every choice, so running
  the script with no arguments always produces byte-identical output.
* Byte-unique files: linters that deduplicate identical document contents in
  their cache must not be able to skip any file.  Each file gets a unique
  numbered title, unique section numbering, and a deterministically shuffled
  paragraph order so no two files share the same bytes.
* Realistic rule-firing density: the paragraph pool is mostly clean, natural
  technical prose.  A second pool of "noisy" paragraphs intentionally exercises
  the patterns that ``textlint-rule-preset-ja-technical-writing`` targets (long
  sentences, too many commas, weak expressions, redundant expressions,
  desumasu/dearu mixing, doubled particles, ra-nuki, double negation, doubled
  conjunctions, repeated words, full/half-width and !? mixing).  Noisy
  paragraphs are injected at a realistic frequency, not in every sentence.

Standard library only (Python 3).
"""

from __future__ import annotations

import os
import random
import shutil

SEED = 20260617

# Target totals.  Tuned so the corpus lands in the 2.5-3.5 MiB window with
# 24-40 files (roughly 80-130 KiB each).
NUM_FILES = 30
# Number of "body tiles" (section + paragraph block) emitted per file.  Each
# tile pulls several paragraphs from the pool; the count is tuned for file size.
TILES_PER_FILE = 56

OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "corpus")

# ---------------------------------------------------------------------------
# Topics.  Each file is anchored to one primary topic (for its title) but pulls
# paragraphs from the shared pool so content stays varied.
# ---------------------------------------------------------------------------
TOPICS = [
    "API設計",
    "デプロイ戦略",
    "テスト自動化",
    "データベース運用",
    "セキュリティ設計",
    "性能チューニング",
    "監視と可観測性",
    "CI/CDパイプライン",
    "認証と認可",
    "ロギング基盤",
    "キャッシュ戦略",
    "非同期処理",
    "型システム活用",
    "メモリ管理",
    "分散システム設計",
    "マイクロサービス",
    "コンテナ運用",
    "インフラ自動化",
    "障害対応",
    "リリース管理",
    "データパイプライン",
    "イベント駆動アーキテクチャ",
    "スキーマ進化",
    "負荷試験",
    "バックアップと復旧",
]

# ---------------------------------------------------------------------------
# Clean paragraph pool.  Each entry is a complete, natural Japanese technical
# paragraph (roughly 150-400 characters).  These dominate the corpus.
# ---------------------------------------------------------------------------
CLEAN_PARAGRAPHS = [
    # --- API design ---
    "API設計では、リソースを名詞として表現し、操作はHTTPメソッドへ対応づける方針が基本となる。"
    "エンドポイントの命名は一貫性を保ち、複数形のリソース名と階層構造で関係を表現する。"
    "後方互換性を維持するため、フィールドの削除は避け、新しい属性は任意項目として追加する。"
    "バージョン番号はURLパスに含め、破壊的変更を導入する場合にのみ新しいバージョンを切る。",

    "エラー応答の形式は全エンドポイントで統一し、機械可読なコードと人間向けのメッセージを併記する。"
    "クライアントが再試行すべきかどうかを判断できるよう、一時的な障害と恒久的な失敗を区別して返す。"
    "ステータスコードは意味に沿って選び、入力検証の失敗には四百番台、サーバ内部の問題には五百番台を用いる。",

    "ページネーションはカーソル方式を採用し、大量のレコードを安定した順序で取得できるようにする。"
    "オフセット方式は途中でデータが挿入されると重複や欠落が生じやすいため、更新頻度の高い一覧には向かない。"
    "応答には次ページを取得するためのトークンを含め、クライアントが状態を保持せずに走査を続けられる構造とする。",

    # --- deployment ---
    "デプロイ戦略としては、新旧の環境を並行して稼働させるブルーグリーン方式が広く用いられる。"
    "切り替えはロードバランサの向き先を変更するだけで完了し、問題が起きた際は即座に旧環境へ戻せる。"
    "一方でカナリアリリースは、ごく一部のトラフィックを新バージョンへ流し、指標を観測しながら段階的に範囲を広げる。",

    "リリース作業を自動化する際は、手順を宣言的に記述し、同じ入力からは常に同じ結果が得られる状態を保つ。"
    "途中で失敗したときに安全に再実行できるよう、各ステップは冪等に設計する。"
    "デプロイの前後で稼働状況を自動的に確認し、異常を検知したら自動的に巻き戻す仕組みを組み込む。",

    # --- testing ---
    "テストはピラミッド構造を意識し、高速で安定した単体テストを土台に据える。"
    "結合テストは外部依存との境界を検証し、エンドツーエンドのテストは主要な利用者の経路だけに絞る。"
    "上層のテストを増やしすぎると実行時間が伸び、原因の特定も難しくなるため、層ごとの役割を明確に分ける。",

    "テストの再現性を確保するには、時刻や乱数、外部サービスといった非決定的な要素を制御下に置く必要がある。"
    "固定のシードや擬似的な時計を注入し、テスト実行のたびに結果が変わらないようにする。"
    "外部サービスへの依存はスタブやフェイクで置き換え、ネットワークの状態に左右されない構成とする。",

    "回帰テストは過去に発生した不具合の再発を防ぐ役割を担う。"
    "障害を修正する際は、まず失敗を再現する最小のテストを書き、それが緑になることを確認してから本体を直す。"
    "こうした手順を徹底すると、同じ原因による不具合が再び混入する確率を大きく下げられる。",

    # --- database ---
    "データベースの索引は検索を高速化する一方で、書き込みのたびに更新の負荷を生む。"
    "そのため索引は実際の問い合わせ傾向に基づいて設計し、使われない索引は定期的に見直して削除する。"
    "複合索引では列の並び順が効きやすさを左右するため、絞り込みに使う列を先頭へ置くことが望ましい。",

    "トランザクションの分離レベルは、整合性と並行性のどちらを優先するかという判断に直結する。"
    "厳しい分離レベルはダーティリードやファントムリードを防ぐが、ロック競合が増えて処理量が落ちやすい。"
    "業務の要求を踏まえ、必要十分な分離レベルを選ぶことが運用上の落としどころとなる。",

    "スキーマ変更を安全に進めるには、列の追加と利用を別々の段階に分けて展開する手法が有効である。"
    "まず後方互換な形で新しい列を追加し、アプリケーションが両方の状態を扱えるようにしてから移行を進める。"
    "古い列への参照がすべて消えたことを確認したうえで、最後に不要となった列を取り除く。",

    # --- security ---
    "セキュリティ設計では、利用者の入力を信頼せず、すべての境界で検証することが原則となる。"
    "想定する形式や範囲を明示的に定義し、それを外れた入力は早い段階で拒否する。"
    "とくに外部から渡された値をそのまま問い合わせや命令に埋め込むと注入攻撃の温床になるため、必ず無害化する。",

    "機密情報を保存する際は、用途に応じた暗号化と鍵管理を組み合わせる。"
    "パスワードは復号できない一方向のハッシュとして保存し、利用者ごとに異なる値を加えて総当たりを困難にする。"
    "鍵は専用の管理機構で保管し、定期的な更新と権限の最小化を徹底する。",

    "権限設計では最小権限の原則を貫き、各構成要素には職務に必要な範囲だけを与える。"
    "広すぎる権限は侵害時の被害を拡大させるため、役割ごとに権限の集合を切り分けて付与する。"
    "付与した権限は棚卸しの対象とし、使われなくなったものを定期的に回収する。",

    # --- performance ---
    "性能改善は計測から始めるべきであり、推測に基づいて手を入れると的外れな最適化に陥りやすい。"
    "実際の負荷の下で処理時間の内訳を取得し、最も時間を消費している部分を特定する。"
    "そのうえで影響の大きい箇所から順に手を入れ、改善のたびに再計測して効果を確かめる。",

    "応答時間を語るときは平均値だけでなく分布の裾を見る必要がある。"
    "平均が良好でも、上位数パーセントの遅い応答が利用者の体験を大きく損なうことは珍しくない。"
    "そのため、九十九パーセンタイルのような裾の指標を監視し、外れ値の原因を継続的に取り除く。",

    "キャッシュは性能を底上げする強力な手段だが、無効化の難しさが本質的な課題として残る。"
    "古い値を返し続けないよう、有効期限と更新契機を明確に定める。"
    "更新の頻度や許容できる遅延を踏まえ、書き込み時に破棄するか、参照時に再計算するかを使い分ける。",

    # --- observability ---
    "可観測性は、ログと指標と分散トレースという三つの柱で支えられる。"
    "指標は全体の傾向を素早く把握するのに適し、ログは個々の事象を詳細に記録する。"
    "分散トレースは一つの要求が複数のサービスを横断する経路を可視化し、遅延の所在を突き止める助けになる。",

    "監視のしきい値は、過去の実績に基づいて現実的な水準に設定する。"
    "厳しすぎると誤った警報が頻発し、運用者が警報そのものを軽視するようになる。"
    "逆に緩すぎると重大な異常を見逃すため、誤報と見逃しの均衡を継続的に調整する。",

    "アラートは行動につながる情報だけに絞り込むことが重要である。"
    "受け取った担当者が何をすべきか判断できないアラートは、混乱を招くだけで価値を生まない。"
    "各アラートには想定される原因と初動の手順を結びつけ、対応の指針を添えておく。",

    # --- CI/CD ---
    "継続的インテグレーションでは、変更を小さくこまめに統合し、早い段階で破綻を検出する。"
    "結合の間隔が空くほど競合の解消は難しくなり、不具合の原因も追いにくくなる。"
    "自動化された検査を各変更に対して実行し、緑の状態を常に保つことを開発の規律とする。",

    "パイプラインは段階を分けて構成し、軽い検査を先に、重い検査を後に配置する。"
    "前段で素早く失敗を返すことで、開発者は手戻りを最小限に抑えられる。"
    "成果物は一度だけ生成し、同じ生成物を各環境へ昇格させることで環境差による不一致を防ぐ。",

    "ビルドの再現性を保つには、依存関係の版を固定し、外部の状態に左右されない構成を整える。"
    "同じ入力から常に同じ成果物が得られる状態は、調査と監査の両面で大きな価値を持つ。"
    "依存の更新は定期的にまとめて行い、変更の影響範囲を把握しやすくする。",

    # --- auth ---
    "認証は利用者が本人であることを確かめる手続きであり、認可は許可された操作の範囲を判断する手続きである。"
    "両者は混同されがちだが、設計上は明確に分離して扱うべきである。"
    "認証で得た主体の情報をもとに、各操作の認可を一貫した方針で判定する構造が望ましい。",

    "トークンを用いた認証では、有効期限を短く保ち、漏えい時の被害を抑える。"
    "長期間有効な資格情報は便利な反面、流出した際の影響が大きい。"
    "更新用のトークンを別に用意し、短命のアクセストークンを定期的に再発行する方式が一般的である。",

    # --- logging ---
    "ログは構造化された形式で出力し、後から機械的に集計や検索ができるようにする。"
    "自由記述の文字列だけでは抽出が難しいため、要求識別子や利用者識別子といった主要な属性を独立した項目として残す。"
    "一貫した形式を保つことで、複数のサービスにまたがる調査が格段に容易になる。",

    "ログには機密情報を残さないよう、出力の段階で確実に除去する。"
    "認証情報や個人を特定できる値が紛れ込むと、ログ基盤そのものが新たな漏えい経路になりかねない。"
    "出力前の検閲処理を共通化し、開発者が個別に配慮しなくても安全が保たれる仕組みを整える。",

    # --- cache ---
    "分散環境のキャッシュでは、各ノードが保持する内容の一貫性が課題となる。"
    "ある場所で更新した値が別の場所から古いまま参照されると、利用者ごとに見える状態が食い違う。"
    "更新を全ノードへ伝播させるか、十分に短い有効期限を設けるかして、ずれが長く残らないようにする。",

    "キャッシュの破棄が一斉に起きると、背後のデータ源へ要求が殺到して過負荷を招く。"
    "この雪崩現象を避けるため、有効期限にばらつきを持たせ、失効の時刻を分散させる。"
    "あわせて、再計算中の重複要求をまとめる仕組みを入れると、瞬間的な集中をさらに和らげられる。",

    # --- async ---
    "非同期処理では、待ち時間の長い入出力の最中に別の作業を進め、資源を遊ばせない構造を作る。"
    "ただし処理の流れが分断されるため、誤りの伝播や順序の管理が同期処理よりも複雑になる。"
    "失敗をどこで捕捉し、どのように呼び出し元へ伝えるかを、設計の早い段階で定めておく必要がある。",

    "メッセージキューを介した処理では、同じメッセージが複数回届く可能性を前提に設計する。"
    "受信側は冪等に作られ、重複した配送を受け取っても結果が変わらないようにする。"
    "処理に失敗したメッセージは再試行の対象とし、一定回数を超えたものは隔離して人手の調査に回す。",

    # --- type system ---
    "型システムは、表現できる状態の集合を制限することで、誤った状態を最初から作れなくする。"
    "値が取りうる範囲を型で表すと、実行時にしか分からなかった不整合を翻訳の段階で捉えられる。"
    "型を丁寧に設計するほど、誤りの混入する余地は小さくなり、変更時の安心感も増す。",

    "省略可能な値は専用の型で明示し、存在しない場合の扱いを呼び出し側へ強制する。"
    "空を意味する特別な値を放置すると、参照時の失敗が思わぬ場所で表面化する。"
    "型の上で空の可能性を可視化し、各分岐で漏れなく処理することを言語の仕組みに委ねる。",

    # --- memory ---
    "メモリ管理では、確保した資源を確実に解放することが安定稼働の前提となる。"
    "解放を忘れると使用量が時間とともに増え続け、やがて処理が滞る。"
    "所有権を明確にし、誰がいつ解放するかを設計上はっきりさせることで、こうした漏れを防ぐ。",

    "大量のオブジェクトを短時間に生成すると、回収処理の負荷が無視できなくなる。"
    "再利用できる領域をあらかじめ確保しておくと、確保と解放の往復を減らせる。"
    "割り当ての傾向を計測し、寿命の近いものをまとめて扱うことで、回収の頻度と停止時間を抑えられる。",

    # --- distributed ---
    "分散システムでは、ネットワークがいつでも遅延し分断されうるという前提に立って設計する。"
    "ある瞬間の全体像を一つの場所で正確に把握することはできないため、部分的な情報から判断する仕組みが要る。"
    "障害を例外ではなく日常として扱い、一部の故障が全体の停止につながらない構造を目指す。",

    "整合性と可用性は、ネットワークが分断された状況では同時には満たしにくい。"
    "どちらを優先するかは業務の性質によって決まり、決済のような領域では整合性を、"
    "閲覧中心の領域では可用性を選ぶといった判断が現実的である。",

    "合意形成の仕組みは、複数のノードが同じ決定に至ることを保証する。"
    "過半数の同意を必要とする方式は、一部のノードが停止しても処理を続けられる耐性を持つ。"
    "ただし合意には通信の往復が伴うため、応答性とのつり合いを意識して構成する。",

    # --- microservices / containers / infra ---
    "サービスを細かく分割すると、各チームが独立して開発と配備を進めやすくなる。"
    "一方で、サービス間の通信が増えるほど、全体の挙動を見通すことは難しくなる。"
    "分割の境界は組織や業務の区切りに沿わせ、過度な細分化による運用負担の増大を避ける。",

    "コンテナは、アプリケーションとその依存をひとまとめにして配布する手段を提供する。"
    "開発環境と本番環境の差異を縮められるため、環境固有の不具合を減らせる。"
    "ただし、コンテナ自体の構成や基盤の設定が肥大化しないよう、継続的な整理が欠かせない。",

    "インフラを宣言的に記述すると、現在の構成が記録として残り、変更の履歴を追える。"
    "手作業による設定変更は再現性を損ない、環境ごとの差異を生む温床になる。"
    "構成の定義を版管理に置き、検査と承認を経てから適用する流れを整えることが望ましい。",

    # --- incident / release / data pipeline ---
    "障害対応では、まず影響範囲を把握し、原因の追究よりも先に利用者への影響を止めることを優先する。"
    "暫定的な回避策で被害を抑えてから、落ち着いて根本原因の調査に移る。"
    "対応後には経緯を振り返り、同種の障害を防ぐための改善点を記録として残す。",

    "データパイプラインでは、各段階の入出力の形式を明確に定め、途中での破損を早期に検出する。"
    "上流の変更が下流へ波及しやすいため、形式の取り決めを契約として扱い、互換性を慎重に管理する。"
    "再処理が必要になった場合に備え、各段階の中間結果を保持しておくと復旧が容易になる。",

    "バックアップは取得するだけでなく、復元できることを定期的に確かめてはじめて意味を持つ。"
    "復元の手順を試さないまま運用を続けると、いざというときに使えない事態を招く。"
    "取得の間隔と保持期間は、許容できる損失量と復旧目標の時間から逆算して定める。",
]

# ---------------------------------------------------------------------------
# Noisy paragraph pool.  Each entry intentionally triggers one or more rules of
# textlint-rule-preset-ja-technical-writing.  These are injected sparingly so
# the corpus stays realistic rather than error-saturated.
# ---------------------------------------------------------------------------
NOISY_PARAGRAPHS = [
    # long sentence (>100 chars in one sentence) + too many "、"
    "この設計方針については、関係する各チームの意見を集約し、過去の障害事例を踏まえ、"
    "将来的な拡張の余地を残しつつ、運用上の負担も考慮し、段階的に合意形成を進めていく必要があると、"
    "私たちは現時点で考えている。",

    # weak expression (〜かもしれない / 〜と思われる)
    "この変更によって応答時間が改善するかもしれないが、計測の条件によっては効果が小さいかもしれない。"
    "原因は接続の確立にあると思われるものの、確証はまだ得られていない。",

    # redundant expression (〜することができる / 〜することが可能)
    "この機能を使うことで、利用者は設定を細かく調整することができる。"
    "管理画面からは権限の付与と剥奪を行うことが可能であり、操作の履歴も確認することができる。",

    # desumasu / dearu mixing
    "本節では設定ファイルの書き方を説明します。"
    "各項目は階層構造で表現され、上位の設定が下位の設定を上書きする。"
    "詳細は次の章で扱いますが、まずは基本的な構造を理解しておくことが大切である。",

    # doubled particle (私は彼は) + redundant
    "私は彼はこの方式に賛成だと考えている。"
    "実装の負担を減らすことができるという点で、この案は検討する価値があると思われる。",

    # ra-nuki (見れる / 食べれる) + weak
    "管理画面からはログの一覧が見れるようになっている。"
    "ただし権限がない利用者には、詳細までは見れないかもしれない。",

    # double negation (〜ないわけではない)
    "この設定を変更しても、性能に影響がないわけではない。"
    "効果が出ないとは言い切れないが、期待したほどの改善は見込めないかもしれない。",

    # doubled conjunction (しかし...しかし)
    "新しい索引を追加した。しかし、書き込みの負荷が増えてしまった。"
    "しかし、検索は確かに速くなったため、全体としては許容できる範囲だと考えている。",

    # repeated word (同一単語の連続) + too many ASCII commas
    "設定設定の項目は多岐にわたる。"
    "host, port, timeout, retry, backoff といった値を、環境ごとに、慎重に、調整していく。",

    # exclamation / question mark + full/half mixing
    "この挙動は本当に正しいのでしょうか?"
    "設定を見直したところ、想定外の値が入っていました!"
    "環境変数の APP_ENV と app_env が混在していたことが原因のようです。",

    # long + weak + redundant combined
    "本機能の導入によって運用の手間を減らすことができると考えられるが、"
    "実際の効果は環境や利用状況に依存するため、現段階では確実なことは言えないような気がする、"
    "という慎重な見方を私たちは持っている。",

    # doubled conjunction (また...また) + repeated word
    "また、設定の検証を強化した。また、検証検証の結果を記録するようにした。"
    "これにより、誤った設定の混入を早い段階で防げるようになった。",
]

# Connective sentences used to top-and-tail tiles so the prose reads naturally.
LEAD_INS = [
    "ここでは、運用の現場で繰り返し問われてきた論点を整理する。",
    "以下では、設計上の判断とその背景を順を追って述べる。",
    "本節の目的は、実装に着手する前に押さえるべき前提を共有することにある。",
    "実務で意思決定を迫られる場面を想定し、考え方の指針を示す。",
    "この章では、具体的な手順よりも判断の基準に重点を置いて説明する。",
    "現場での経験から得られた知見を、再利用しやすい形でまとめる。",
]

WRAP_UPS = [
    "以上を踏まえ、次節では具体的な構成例を取り上げる。",
    "ここまでの議論は、後続の設計判断の土台となる。",
    "詳細な設定値は付録に整理したので、必要に応じて参照してほしい。",
    "この方針は固定的なものではなく、状況に応じて見直す前提で運用する。",
    "実際の適用にあたっては、組織の事情に合わせた調整が欠かせない。",
]


def section_title(rng: random.Random, topic: str, index: int) -> str:
    """Build a section heading that varies per file and per tile."""
    aspects = [
        "全体像と前提",
        "設計上の判断",
        "実装の指針",
        "運用での注意点",
        "よくある落とし穴",
        "計測と検証",
        "段階的な移行",
        "失敗からの学び",
        "チーム間の合意形成",
        "今後の課題",
        "基本方針の確認",
        "実例による考察",
    ]
    aspect = aspects[index % len(aspects)]
    return f"{index}. {topic}における{aspect}"


def build_document(rng: random.Random, file_index: int) -> str:
    """Construct a single Markdown document as a unique byte string."""
    primary_topic = TOPICS[file_index % len(TOPICS)]
    doc_id = f"doc_{file_index:03d}"

    lines: list[str] = []
    # Unique title: numbered id + primary topic guarantees uniqueness across
    # files even before paragraph shuffling.
    lines.append(f"# {doc_id}: {primary_topic}に関する技術ノート")
    lines.append("")
    lines.append(
        f"本ドキュメント（整理番号 {doc_id}）は、{primary_topic}を中心に、"
        "関連する設計と運用の論点を技術文書としてまとめたものである。"
    )
    lines.append("")

    # A per-file deterministic permutation of the clean pool drives paragraph
    # selection, ensuring distinct ordering between files.
    clean_order = list(range(len(CLEAN_PARAGRAPHS)))
    rng.shuffle(clean_order)
    clean_cursor = 0

    for tile in range(1, TILES_PER_FILE + 1):
        # Rotate the topic per section so each file covers varied ground.
        section_topic = TOPICS[(file_index + tile) % len(TOPICS)]
        lines.append("")
        lines.append(f"## {section_title(rng, section_topic, tile)}")
        lines.append("")
        lines.append(rng.choice(LEAD_INS))
        lines.append("")

        # Pull a few clean paragraphs from the shuffled order (wrapping around).
        paragraphs_in_tile = rng.randint(3, 4)
        for _ in range(paragraphs_in_tile):
            idx = clean_order[clean_cursor % len(clean_order)]
            clean_cursor += 1
            lines.append(CLEAN_PARAGRAPHS[idx])
            lines.append("")

            # Inject a noisy paragraph at a realistic frequency (~25%).
            if rng.random() < 0.25:
                lines.append(rng.choice(NOISY_PARAGRAPHS))
                lines.append("")

        lines.append(rng.choice(WRAP_UPS))
        lines.append("")

    # Trailing per-file unique marker comment so even files that happened to
    # select the same paragraphs differ by bytes.  Kept as plain prose so it
    # does not skew Markdown-specific linters.
    lines.append("")
    lines.append(
        f"（この節の整理番号は {doc_id} であり、主題は{primary_topic}である。"
        "本稿の構成と内容は、同一コーパス内の他の文書とは独立している。）"
    )
    lines.append("")

    return "\n".join(lines)


def main() -> None:
    rng = random.Random(SEED)

    os.makedirs(OUT_DIR, exist_ok=True)
    # Clear existing generated files (doc_*.md) so re-runs start clean.
    for name in os.listdir(OUT_DIR):
        if name.startswith("doc_") and name.endswith(".md"):
            os.remove(os.path.join(OUT_DIR, name))

    sizes: list[int] = []
    seen_bytes: set[bytes] = set()

    for i in range(1, NUM_FILES + 1):
        text = build_document(rng, i)
        data = text.encode("utf-8")
        if data in seen_bytes:
            # Should never happen given unique titles/markers, but guard anyway.
            raise RuntimeError(f"duplicate byte content generated for doc_{i:03d}")
        seen_bytes.add(data)

        path = os.path.join(OUT_DIR, f"doc_{i:03d}.md")
        with open(path, "wb") as fh:
            fh.write(data)
        sizes.append(len(data))

    total = sum(sizes)
    print(f"files:        {len(sizes)}")
    print(f"total bytes:  {total} ({total / (1024 * 1024):.3f} MiB)")
    print(f"min file:     {min(sizes)} bytes ({min(sizes) / 1024:.1f} KiB)")
    print(f"max file:     {max(sizes)} bytes ({max(sizes) / 1024:.1f} KiB)")
    print(f"avg file:     {total // len(sizes)} bytes ({total / len(sizes) / 1024:.1f} KiB)")
    print(
        f"pools:        clean={len(CLEAN_PARAGRAPHS)} noisy={len(NOISY_PARAGRAPHS)} "
        f"(total paragraph pool={len(CLEAN_PARAGRAPHS) + len(NOISY_PARAGRAPHS)})"
    )


if __name__ == "__main__":
    main()
